use serde::Serialize;
use std::sync::Mutex;
use sysinfo::System;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub cpu_usage: f64,
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
    pub mem_percent: f64,
}

/// 全局 System 实例：CPU 使用率需要两次采样差值计算，跨命令调用保持状态
static SYS: Mutex<Option<System>> = Mutex::new(None);

/// 读取本机 CPU / 内存用量
#[tauri::command]
pub fn get_system_info() -> SystemInfo {
    let mut sys = match SYS.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    let sys = sys.get_or_insert_with(System::new);

    // CPU 使用率是两次 refresh 之间的差值：前端每 2s 轮询一次，
    // 首次调用返回 0，之后即真实的 2s 窗口均值，无需额外 sleep。
    sys.refresh_cpu_usage();

    let cpu_usage = sys.global_cpu_usage() as f64;

    sys.refresh_memory();
    let mem_total_mb = sys.total_memory() / 1024 / 1024;
    let mem_used_mb = sys.used_memory() / 1024 / 1024;
    let mem_percent = if mem_total_mb > 0 {
        (mem_used_mb as f64 / mem_total_mb as f64) * 100.0
    } else {
        0.0
    };

    log::debug!(
        "读取系统资源: cpu={:.1}% mem={}/{}MB ({:.1}%)",
        cpu_usage,
        mem_used_mb,
        mem_total_mb,
        mem_percent
    );
    SystemInfo {
        cpu_usage,
        mem_used_mb,
        mem_total_mb,
        mem_percent,
    }
}
