use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;
use sysinfo::{Networks, System};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub cpu_usage: f64,
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
    pub mem_percent: f64,
    /// 上行速率（字节/秒，本次采样窗口均值）
    pub net_upload: f64,
    /// 下行速率（字节/秒）
    pub net_download: f64,
    /// 上行 + 下行合计速率（字节/秒）
    pub net_total_speed: f64,
    /// 总流量：应用开始监控以来所有网卡收发累计（字节），重启归零
    pub net_total_traffic: u64,
    /// 逻辑 CPU 线程数
    pub cpu_threads: u32,
}

/// 全局 System 实例：CPU 使用率需要两次采样差值计算，跨命令调用保持状态
static SYS: Mutex<Option<System>> = Mutex::new(None);

/// 全局网卡监控：速率 = 两次 refresh_list 之间的字节数差 ÷ 实际流逝秒数
static NET: Mutex<Option<Networks>> = Mutex::new(None);
static NET_LAST: Mutex<Option<Instant>> = Mutex::new(None);

/// 总流量基线表：网卡名 → (基线计数, 最近计数, 已入账流量)。
///
/// sysinfo 的 `total_received()/total_transmitted()` 是**系统开机以来**的绝对计数
/// （网卡首次入表时赋原始 InOctets/OutOctets），直接求和会让「总流量」在应用刚启动
/// 就显示几十 GB、重启也不归零。按网卡记「本应用会话首次见到时的计数」作基线，
/// 展示值 = 最近 − 基线 + 已入账，从 0 起算、重启归零。
static NET_BASE: Mutex<Option<HashMap<String, (u64, u64, u64)>>> = Mutex::new(None);

/// 由本轮在线网卡样本推进基线表，返回应用启动以来的总流量（字节）。纯函数便于单测。
///
/// - 首次见到的网卡：基线 = 当前计数，贡献从 0 起算（新插网卡/应用重启都是这个语义）；
/// - 本轮缺席（断网/切 VPN，sysinfo 会把没刷到的网卡整条移出列表）：不删表项，
///   按最近计数冻结参与求和——总数不因断网回跳；
/// - 重现后计数变小（系统重置了该网卡的计数器）：把已有贡献转进「已入账」，
///   基线重新取当前值——贡献无缝续算，全程单调不降。
fn total_traffic_since_start(
    seen: &mut HashMap<String, (u64, u64, u64)>,
    samples: &[(String, u64)],
) -> u64 {
    for (name, now) in samples {
        let (base, last, retired) = seen.entry(name.clone()).or_insert((*now, *now, 0));
        if *now < *last {
            *retired += *last - *base;
            *base = *now;
        }
        *last = *now;
    }
    seen.values()
        .map(|(base, last, retired)| retired + last.saturating_sub(*base))
        .sum()
}

/// 汇总所有网卡的上下行速率与累计流量。速率按真实流逝时间折算，
/// 前端失焦切到 5s 慢轮询档时依然准确；首采窗口过短（<100ms）按 0 处理，
/// 与 CPU 首采为 0 同一口径。总流量按基线表折算为本应用启动以来的累计，
/// 重启归零、断网不回跳（见 total_traffic_since_start）。
fn sample_network() -> (f64, f64, f64, u64) {
    let mut net = match NET.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let net = net.get_or_insert_with(Networks::new_with_refreshed_list);
    net.refresh_list();

    let now = Instant::now();
    let elapsed = NET_LAST
        .lock()
        .ok()
        .and_then(|last| *last)
        .map(|t| now.duration_since(t).as_secs_f64());
    if let Ok(mut last) = NET_LAST.lock() {
        *last = Some(now);
    }

    let mut up = 0u64;
    let mut down = 0u64;
    let mut samples: Vec<(String, u64)> = Vec::new();
    for (name, data) in net.list() {
        down += data.received();
        up += data.transmitted();
        samples.push((name.clone(), data.total_received() + data.total_transmitted()));
    }
    let mut base = match NET_BASE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let total = total_traffic_since_start(base.get_or_insert_with(HashMap::new), &samples);

    match elapsed {
        Some(secs) if secs >= 0.1 => {
            let rate = |bytes: u64| bytes as f64 / secs;
            (rate(up), rate(down), rate(up + down), total)
        }
        _ => (0.0, 0.0, 0.0, total),
    }
}

/// 读取本机 CPU / 内存 / 网络用量
#[tauri::command]
pub fn get_system_info() -> SystemInfo {
    let mut sys = match SYS.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    let sys = sys.get_or_insert_with(System::new);

    // CPU 使用率是两次 refresh 之间的差值：前端每 2s 轮询一次，
    // 首次调用返回 0，之后即真实的 2s 窗口均值，无需额外 sleep。
    // refresh_cpu_usage 首次调用即填充 CPU 列表，线程数随之可得。
    sys.refresh_cpu_usage();

    let cpu_usage = sys.global_cpu_usage() as f64;
    let cpu_threads = sys.cpus().len() as u32;

    sys.refresh_memory();
    let mem_total_mb = sys.total_memory() / 1024 / 1024;
    let mem_used_mb = sys.used_memory() / 1024 / 1024;
    let mem_percent = if mem_total_mb > 0 {
        (mem_used_mb as f64 / mem_total_mb as f64) * 100.0
    } else {
        0.0
    };

    let (net_upload, net_download, net_total_speed, net_total_traffic) = sample_network();

    log::debug!(
        "读取系统资源: cpu={:.1}% mem={}/{}MB ({:.1}%) net=↑{:.0} ↓{:.0}B/s",
        cpu_usage,
        mem_used_mb,
        mem_total_mb,
        mem_percent,
        net_upload,
        net_download
    );
    SystemInfo {
        cpu_usage,
        mem_used_mb,
        mem_total_mb,
        mem_percent,
        net_upload,
        net_download,
        net_total_speed,
        net_total_traffic,
        cpu_threads,
    }
}

#[cfg(test)]
mod tests {
    use super::total_traffic_since_start;
    use std::collections::HashMap;

    /// 首次见到网卡：基线 = 当前绝对计数，贡献为 0——卡片不会一打开就显示
    /// 系统开机以来的几十 GB，而是从 0 起算；之后只累计增量。
    #[test]
    fn first_sight_starts_at_zero() {
        let mut seen = HashMap::new();
        let t = total_traffic_since_start(&mut seen, &[("以太网".into(), 1_000_000_000)]);
        assert_eq!(t, 0, "首采按基线归零");
        let t = total_traffic_since_start(&mut seen, &[("以太网".into(), 1_000_001_500)]);
        assert_eq!(t, 1_500, "只算基线之后的增量");
    }

    /// 网卡缺席（断开/切 VPN）按最近计数冻结，总数不回跳；重现后计数器被系统
    /// 重置时把贡献转「已入账」无缝续算；新网卡从 0 起算不影响已有累计。
    #[test]
    fn missing_nic_freezes_and_counter_reset_continues() {
        let mut seen = HashMap::new();
        total_traffic_since_start(&mut seen, &[("wlan".into(), 100)]);
        let t = total_traffic_since_start(&mut seen, &[("wlan".into(), 300)]);
        assert_eq!(t, 200);
        // wlan 断开从列表消失：冻结在 200，不回跳
        let t = total_traffic_since_start(&mut seen, &[]);
        assert_eq!(t, 200, "缺席网卡按最近计数冻结");
        // 重连后系统把该网卡计数器清零重数（50 < 最近 300）：贡献从 200 续算
        let t = total_traffic_since_start(&mut seen, &[("wlan".into(), 50)]);
        assert_eq!(t, 200, "计数器重置不丢已入账的 200");
        let t = total_traffic_since_start(&mut seen, &[("wlan".into(), 80)]);
        assert_eq!(t, 230, "重置后的增量照常累加");
        // 第二块网卡首次出现：从 0 起算，已有累计不受影响
        let t = total_traffic_since_start(
            &mut seen,
            &[("wlan".into(), 100), ("eth".into(), 5_000)],
        );
        assert_eq!(t, 250, "新网卡贡献 0，wlan 续算到 250");
    }
}
