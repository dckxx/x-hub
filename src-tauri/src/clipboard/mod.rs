//! 剪贴板历史：按平台分发实现。
//!
//! - Windows：原生 Win32（事件驱动监听 + 无激活浮层 + SendInput 粘贴）
//! - Linux/macOS：arboard 读写 + 轮询监听 + Tauri 浮层

#[cfg(target_os = "windows")]
#[path = "windows.rs"]
mod platform;

#[cfg(not(target_os = "windows"))]
#[path = "linux.rs"]
mod platform;

pub use platform::*;
