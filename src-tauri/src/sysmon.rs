use serde::Serialize;
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

/// 汇总所有网卡的上下行速率与累计流量。速率按真实流逝时间折算，
/// 前端失焦切到 5s 慢轮询档时依然准确；首采窗口过短（<100ms）按 0 处理，
/// 与 CPU 首采为 0 同一口径。总流量为 sysinfo 监控开始以来的累计，重启归零。
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
    let mut total = 0u64;
    for data in net.list().values() {
        down += data.received();
        up += data.transmitted();
        total += data.total_received() + data.total_transmitted();
    }

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
