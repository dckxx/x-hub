use crate::commands::DbState;
use tauri::{AppHandle, Emitter, Manager};

/// 到点判定阈值（毫秒）：到点超过该值视为「错过」（应用关闭/休眠期间），只标记已触发不补发
const MISSED_GRACE_MS: i64 = 5_000;

/// 待办提醒后台线程：每秒扫描 remind_at 到期且未触发的未完成待办，
/// 到点发系统通知 + emit `todo-remind` 事件（前端 toast），并置 remind_fired 防重复。
/// 与倒计时一致：完全退出/休眠期间错过的提醒（超 5s）静默跳过不补发。
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        loop {
            let _ = tick_once(&app);
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    });
    log::info!("待办提醒线程已启动");
}

fn tick_once(app: &AppHandle) -> Result<(), String> {
    let Some(state) = app.try_state::<DbState>() else {
        return Ok(());
    };
    let now = crate::repo::countdown::now_ms();

    // 先取出全部到期项，立即释放锁，避免通知/事件期间阻塞数据库
    let due = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        crate::repo::todo::list_due_reminders(&conn, now).map_err(|e| e.to_string())?
    };

    for item in due {
        let remind_at = item.remind_at.unwrap_or(now);
        let missed = now - remind_at > MISSED_GRACE_MS;

        {
            let conn = state.0.lock().map_err(|e| e.to_string())?;
            crate::repo::todo::mark_remind_fired(&conn, item.id).map_err(|e| e.to_string())?;
        }

        if !missed {
            let _ = app.emit("todo-remind", &item);
            // 真 Toast 通知（notify.rs），与倒计时提醒同一通道
            crate::notify::show_system_notification(app, &format!("待办提醒 · {}", item.title), "提醒时间到");
            log::info!("待办提醒: id={} title={}", item.id, item.title);
        } else {
            log::info!("待办提醒错过(不补发): id={} title={}", item.id, item.title);
        }
        let _ = app.emit("todos-changed", ());
    }
    Ok(())
}
