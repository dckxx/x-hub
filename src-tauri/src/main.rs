// Prevents additional console window on Windows in release, DO NOT REMOVE!!
// 仅 Windows 生效；macOS 上该子系统属性无意义且会被拒绝，故按目标平台门控。
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

fn main() {
  app_lib::run();
}
