use crate::commands::DbState;
use crate::models::Countdown;
use tauri::{AppHandle, Emitter, Manager};

/// 到点判定阈值（毫秒）：到点超过该值视为「错过」（应用关闭/休眠期间），只顺延不补发
const MISSED_GRACE_MS: i64 = 5_000;

/// 后台倒计时驱动线程：每秒扫描一次到期项，
/// 到点发系统通知 + emit `countdown-fired` 事件，然后按模式推进 end_at。
/// 完全退出期间错过的提醒（end_at 已远落后于 now）静默顺延，不补发。
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        loop {
            let _ = tick_once(&app);
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    });
    log::info!("倒计时驱动线程已启动");
}

fn tick_once(app: &AppHandle) -> Result<(), String> {
    let Some(state) = app.try_state::<DbState>() else {
        return Ok(());
    };
    let now = crate::repo::countdown::now_ms();

    // 先取出全部到期项，立即释放锁，避免通知/事件期间阻塞数据库
    let due = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        crate::repo::countdown::list_due(&conn, now).map_err(|e| e.to_string())?
    };

    for item in due {
        let missed = now - item.end_at > MISSED_GRACE_MS;
        let next = {
            let conn = state.0.lock().map_err(|e| e.to_string())?;
            advance(&conn, &item, now, missed)?
        };

        if !missed {
            // 到点：系统通知 + 前端事件（toast / 声音）
            let _ = send_notification(app, &item);
            let _ = app.emit("countdown-fired", &item);

            // once 到点自动关闭浮窗（水排空即收工）
            if item.repeat_mode == "once" && item.floated {
                let conn = state.0.lock().map_err(|e| e.to_string())?;
                let _ = crate::repo::countdown::set_floated(&conn, item.id, false, None, None);
                drop(conn);
                crate::countdown_window::destroy(app, item.id);
            }
        } else {
            log::info!(
                "倒计时错过顺延(不提醒): id={} name={} 原到点={} 下次={}",
                item.id,
                item.name,
                item.end_at,
                next
            );
        }

        let _ = app.emit("countdowns-changed", ());
    }
    Ok(())
}

/// 计算到点后的下一次 end_at：
/// - once：到点即 finished（返回原 end_at，仅标记结束）
/// - daily：+24h
/// - interval：按间隔顺延到未来（跳过错过的轮次）
fn advance(conn: &rusqlite::Connection, item: &Countdown, now: i64, missed: bool) -> Result<i64, String> {
    match item.repeat_mode.as_str() {
        "once" => {
            if missed {
                // 关闭期间错过的定时：静默标记结束，不补发
                crate::repo::countdown::mark_finished(conn, item.id).map_err(|e| e.to_string())?;
            } else {
                // 运行中到点：若已浮窗，浮窗随后端事件更新；主卡标记灰态
                crate::repo::countdown::mark_finished(conn, item.id).map_err(|e| e.to_string())?;
                log::info!("倒计时到点(once): id={} name={}", item.id, item.name);
            }
            Ok(item.end_at)
        }
        "daily" => {
            let mut t = item.end_at;
            while t <= now {
                t += 24 * 60 * 60 * 1000;
            }
            crate::repo::countdown::advance(conn, item.id, t).map_err(|e| e.to_string())?;
            if !missed {
                log::info!("倒计时到点(daily): id={} name={} 下次={}", item.id, item.name, t);
            }
            Ok(t)
        }
        _ => {
            let interval = item.interval_minutes.unwrap_or(1).max(1) * 60 * 1000;
            let mut t = item.end_at;
            while t <= now {
                t += interval;
            }
            crate::repo::countdown::advance(conn, item.id, t).map_err(|e| e.to_string())?;
            if !missed {
                log::info!("倒计时到点(interval): id={} name={} 下次={}", item.id, item.name, t);
            }
            Ok(t)
        }
    }
}

fn send_notification(app: &AppHandle, item: &Countdown) -> Result<(), String> {
    // 便携版无安装器，系统 toast 依赖 AUMID 注册会静默失败；
    // 统一走托盘气泡（Win10/11 渲染为操作中心的系统级通知）。
    let title = format!("倒计时提醒 · {}", item.name);
    let body = match item.repeat_mode.as_str() {
        "daily" => "每日提醒时间到",
        "interval" => "间隔提醒时间到",
        _ => "时间到",
    };
    crate::notify::show_system_notification(app, &title, body);
    log::info!("已发送系统通知: id={} name={}", item.id, item.name);
    Ok(())
}
