mod account;
mod api_spec;
mod autostart;
mod browsers;
mod app_icon;
mod chat;
mod chat_window;
mod clipboard;
mod commands;
mod config;
mod credentials;
mod countdown_ticker;
mod countdown_window;
mod db;
mod extension;
mod ext_protocol;
mod favicon;
mod floating_ball;
mod float_window;
pub mod market;
mod models;
mod net;
mod note_io;
mod notify;
mod online;
mod paths;
mod precheck;
mod process;
mod proxy;
mod publisher;
mod repo;
mod runtime;
mod secret;
mod service;
mod shortcut;
pub mod signing;
mod skills;
mod suda_browser;
mod sticky_window;
mod sysmon;
mod todo_reminder;
mod todo_recurrence;
mod tray;
pub mod updater;
mod webview_mem;
mod win_taskbar;
mod xhub_api;

/// WebView2 附加浏览器参数（所有窗口必须完全一致——同一 user data folder 下
/// 不同参数的环境创建会失败；tauri.conf.json 主窗的 additionalBrowserArgs 与此
/// 逐字一致，webview_mem.rs 有守卫测试）。
/// 保留 wry 默认的 --disable-features 前缀；曾带 --disable-background-timer-throttling
/// （禁用后台定时器节流），已摘除：隐藏窗口里的 JS 定时器交还浏览器自动节流
/// （钳到 ≥1s、长期隐藏降到每分钟 1 次）兜底，重量级轮询由 useAdaptivePolling
/// 按可见性/聚焦自行门控。到点类「正事」（倒计时/待办提醒）在 Rust 原生线程，
/// 不受此参数影响。
/// 内存优化 P3 追加两项（微软 WebView2 资源优化指南推荐值）：
/// - --js-flags=--scavenger_max_new_space_capacity_mb=8：压 V8 新生代堆上限，
///   降低 JS 引擎常驻内存；若主窗/编辑器出现 GC 卡顿可放宽到 16–32
/// - --disk-cache-size=33554432：磁盘缓存上限 32MB
pub const ADDITIONAL_BROWSER_ARGS: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --js-flags=--scavenger_max_new_space_capacity_mb=8 --disk-cache-size=33554432";

use commands::DbState;
use rusqlite::Connection;
use tauri::{Listener, Manager};

/// 初始化数据库并返回连接
fn init_database() -> Result<Connection, Box<dyn std::error::Error>> {
    let app_data_dir = crate::paths::data_root().to_path_buf();
    std::fs::create_dir_all(&app_data_dir)?;

    // 启动时应用待恢复的数据（恢复命令只暂存，重启后替换）
    apply_pending_restore(&app_data_dir);

    let db_path = app_data_dir.join("app.db");
    let conn = db::init(&db_path)?;
    log::info!("数据库初始化完成: {}", db_path.display());
    Ok(conn)
}

/// 应用待恢复的数据：将 restore.db / restore_icons 替换为正式数据
fn apply_pending_restore(app_data: &std::path::Path) {
    let flag = app_data.join(".restore_pending");
    if !flag.exists() {
        return;
    }
    let db = app_data.join("app.db");
    let restore = app_data.join("restore.db");
    if restore.exists() {
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(app_data.join("app.db-wal"));
        let _ = std::fs::remove_file(app_data.join("app.db-shm"));
        let _ = std::fs::copy(&restore, &db);
        let _ = std::fs::remove_file(&restore);
    }
    let restore_icons = app_data.join("restore_icons");
    if restore_icons.exists() {
        let _ = std::fs::remove_dir_all(app_data.join("icons"));
        let _ = std::fs::rename(&restore_icons, app_data.join("icons"));
    }
    let _ = std::fs::remove_file(&flag);
    log::info!("已应用待恢复的数据");
}

/// 一次性迁移旧目录（com.workbench.desktop）数据到新目录（x-hub）
fn migrate_legacy_data() {
    let new_dir = crate::paths::data_root().to_path_buf();
    if new_dir.exists() {
        return;
    }
    let Some(legacy_dir) = dirs::data_dir().map(|d| d.join("com.workbench.desktop")) else {
        return;
    };
    if !legacy_dir.exists() {
        return;
    }
    if std::fs::create_dir_all(&new_dir).is_err() {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(&legacy_dir) {
        for entry in entries.flatten() {
            let src = entry.path();
            let dst = new_dir.join(entry.file_name());
            if src.is_dir() {
                let _ = copy_dir(&src, &dst);
            } else {
                let _ = std::fs::copy(&src, &dst);
            }
        }
    }
    log::info!("已从旧目录迁移数据: {}", legacy_dir.display());
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let s = entry.path();
        let d = dst.join(entry.file_name());
        if s.is_dir() {
            copy_dir(&s, &d)?;
        } else {
            std::fs::copy(&s, &d)?;
        }
    }
    Ok(())
}

/// 将数据库中旧目录（com.workbench.desktop）的图标路径替换为当前目录（x-hub）
/// 幂等：重复执行无副作用，每次启动调用
fn fix_icon_paths(conn: &Connection) {
    let new_dir = crate::paths::data_root();
    let Some(old_dir) = dirs::data_dir().map(|d| d.join("com.workbench.desktop")) else {
        return;
    };
    let old = old_dir.to_string_lossy().into_owned();
    let new = new_dir.to_string_lossy().into_owned();
    match conn.execute(
        "UPDATE resources SET icon = replace(icon, ?1, ?2) WHERE icon LIKE ?1 || '%'",
        rusqlite::params![old, new],
    ) {
        Ok(n) if n > 0 => log::info!("已修复 {} 条图标路径为 x-hub 目录", n),
        _ => {}
    }
}

/// 验证窗口位置是否在任意可用的显示器内
pub(crate) fn is_position_on_screen(x: f64, y: f64) -> bool {
    // Tauri 2 没有直接枚举显示器的 API，使用一个合理的边界检查
    // 允许负坐标（多显示器配置），但限制在合理范围内
    x >= -10000.0 && x <= 10000.0 && y >= -10000.0 && y <= 10000.0
}

/// 主窗句柄统一入口（ADR 0011 起必须走这里）：
/// 主窗自「速达应用内打开网页」起内嵌了子 webview（suda-panel），tauri 的
/// `get_webview_window` 内部校验 is_webview_window（窗口上所有 webview 的 label
/// 必须都等于窗口 label），多 webview 窗口一律返回 None——按它找主窗会让托盘/
/// 快捷键/悬浮球/关闭拦截/几何保存/浮窗定位全部静默失效（实测踩过）。
/// `get_window` 按窗口注册表查 label，不受窗口上有几个 webview 影响。
pub fn main_window(app: &tauri::AppHandle) -> Option<tauri::Window<tauri::Wry>> {
    app.get_window("main")
}

/// 纯几何：两个物理像素矩形是否相交（窗口是否还落在某块屏幕上）。
/// 边缘恰好相切视为不相交。
fn physical_rects_intersect(
    ax: i32,
    ay: i32,
    aw: u32,
    ah: u32,
    bx: i32,
    by: i32,
    bw: u32,
    bh: u32,
) -> bool {
    ax < bx.saturating_add(bw as i32)
        && bx < ax.saturating_add(aw as i32)
        && ay < by.saturating_add(bh as i32)
        && by < ay.saturating_add(ah as i32)
}

/// 纯几何：把窗口常规尺寸夹进可用区（逻辑像素，预留 16px 边距、极小屏兜底），
/// 小屏启动「先夹后最大化」用——否则还原（restore down）拿回的还是超屏几何。
fn fit_window_size(w: f64, h: f64, avail_w: f64, avail_h: f64) -> (f64, f64) {
    let fit = |v: f64, avail: f64, min: f64| v.min((avail - 16.0).max(min)).max(min);
    (fit(w, avail_w, 320.0), fit(h, avail_h, 240.0))
}

/// 恢复位置完全脱离所有显示器（拔掉副屏残留坐标 / 位置漂移出屏）时，
/// 把窗口搬回主显示器工作区左上角。只救「完全不可见」：与任一屏幕仍有交集的
/// （含负坐标多显示器）一律不动。
fn rescue_offscreen_window(window: &tauri::Window<tauri::Wry>) {
    let Ok(pos) = window.outer_position() else { return };
    let Ok(size) = window.outer_size() else { return };
    let monitors = window.available_monitors().unwrap_or_default();
    let visible = monitors.iter().any(|m| {
        let wa = m.work_area();
        physical_rects_intersect(
            pos.x,
            pos.y,
            size.width,
            size.height,
            wa.position.x,
            wa.position.y,
            wa.size.width,
            wa.size.height,
        )
    });
    if visible {
        return;
    }
    let fallback = window
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| monitors.into_iter().next());
    if let Some(m) = fallback {
        let wa = m.work_area();
        let x = wa.position.x.saturating_add(16);
        let y = wa.position.y.saturating_add(16);
        log::warn!(
            "窗口位置 ({},{}) 不在任何显示器上，移回 {} 工作区 ({},{})",
            pos.x,
            pos.y,
            m.name().map_or("主显示器", |n| n.as_str()),
            x,
            y
        );
        let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    }
}

/// 应用启动时恢复上次保存的窗口位置、尺寸与置顶状态
fn restore_window_state(app: &tauri::App) {
    let config = config::load();
    if let Some(window) = main_window(app.handle()) {
        let ws = &config.window;
        let _ = window.set_size(tauri::LogicalSize::new(ws.width, ws.height));
        if let (Some(x), Some(y)) = (ws.x, ws.y) {
            if is_position_on_screen(x, y) {
                let _ = window.set_position(tauri::LogicalPosition::new(x, y));
            }
        }
        // 位置恢复后先救出屏：is_position_on_screen 的 ±10000 粗检挡不住拔掉
        // 副屏/漂移残留的坐标，而位置完全出屏时 current_monitor 为 None，
        // 连下面的小屏最大化兜底都会被跳过
        rescue_offscreen_window(&window);
        if ws.always_on_top {
            let _ = window.set_always_on_top(true);
        }
        // 小屏适配：屏幕（按窗口当前所在显示器，取不到回退主显示器）容不下
        // 默认 1400×900 时直接最大化启动——否则窗口下半部分掉到屏幕外，没有任何
        // 入口能把窗口拖回来。物理像素先按 DPI 缩放折算成逻辑像素再比较；
        // 比较基准取「默认尺寸与记忆尺寸的较大者」，记忆尺寸更小时也按默认判。
        // monitor 查找必须连 Ok(None)（窗口不在任何屏上）一起兜底回主显示器：
        // .or_else 只兜 Err 不兜 None，None 时整段静默跳过。
        let want_w = ws.width.max(1400.0);
        let want_h = ws.height.max(900.0);
        let monitor = window
            .current_monitor()
            .ok()
            .flatten()
            .or_else(|| window.primary_monitor().ok().flatten());
        if let Some(monitor) = monitor {
            let scale = monitor.scale_factor();
            let logical_w = monitor.size().width as f64 / scale;
            let logical_h = monitor.size().height as f64 / scale;
            if logical_w < want_w || logical_h < want_h {
                log::info!(
                    "屏幕逻辑分辨率 {:.0}x{:.0} 小于窗口 {:.0}x{:.0}，启动即最大化",
                    logical_w,
                    logical_h,
                    want_w,
                    want_h
                );
                // 先把常规几何夹进工作区再最大化：Windows 的还原态记住的是
                // maximize 前的尺寸，直接最大化会让「还原」回到超屏窗口
                let wa = monitor.work_area();
                let (fit_w, fit_h) = fit_window_size(
                    ws.width,
                    ws.height,
                    wa.size.width as f64 / scale,
                    wa.size.height as f64 / scale,
                );
                let _ = window.set_size(tauri::LogicalSize::new(fit_w, fit_h));
                let _ = window.maximize();
            }
        }
        log::info!(
            "恢复窗口状态: {}x{} @ ({:?},{:?}) 置顶={}",
            ws.width,
            ws.height,
            ws.x,
            ws.y,
            ws.always_on_top
        );
    }
}

/// 保存窗口位置与尺寸到配置
fn persist_window_state(app: &tauri::AppHandle) {
    if let Some(window) = main_window(app) {
        if window.is_minimized().unwrap_or(false) {
            return;
        }
        // 最大化态不覆盖记忆的常规几何：保存的是放大后的整屏尺寸，下次启动
        // 会以「非最大化 + 超大窗口」恢复，反而把窗口撑出屏幕
        if window.is_maximized().unwrap_or(false) {
            return;
        }
        if let Ok(pos) = window.outer_position() {
            if let Ok(size) = window.inner_size() {
                // inner_size/outer_position 返回物理像素，落盘前统一折算成逻辑像素：
                // 恢复侧按 LogicalSize/LogicalPosition 解释，直接存物理值会在非 100%
                // 缩放下每关开一轮放大 scale_factor 倍、位置同步向右下漂移
                //（125% 用户窗口逐次涨到超出屏幕即此因，2026-09-29 修）
                let scale = window.scale_factor().unwrap_or(1.0);
                let pos = pos.to_logical::<f64>(scale);
                let size = size.to_logical::<f64>(scale);
                let _guard = config::lock();
                let mut cfg = config::load();
                cfg.window.x = Some(pos.x);
                cfg.window.y = Some(pos.y);
                cfg.window.width = size.width;
                cfg.window.height = size.height;
                match config::save(&cfg) {
                    Ok(()) => log::debug!("窗口状态已保存: {}x{} @ ({},{})", size.width, size.height, pos.x, pos.y),
                    Err(e) => log::warn!("窗口状态保存失败: {}", e),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rects_intersect_covers_overlap_and_miss() {
        // 完全包含
        assert!(physical_rects_intersect(0, 0, 1400, 900, -100, -100, 3000, 2000));
        // 部分重叠
        assert!(physical_rects_intersect(1000, 500, 1400, 900, 1920, 0, 1920, 1080));
        // 负坐标副屏（主屏左侧）
        assert!(physical_rects_intersect(-500, 100, 1400, 900, -1920, 0, 1920, 1080));
        // 完全在屏幕右侧外
        assert!(!physical_rects_intersect(3000, 0, 1400, 900, 0, 0, 1920, 1080));
        // 完全在屏幕下方外
        assert!(!physical_rects_intersect(0, 2000, 1400, 900, 0, 0, 1920, 1080));
        // 边缘恰好相切不算相交
        assert!(!physical_rects_intersect(1920, 0, 1400, 900, 0, 0, 1920, 1080));
    }

    #[test]
    fn fit_size_clamps_only_what_overflows() {
        // 屏内尺寸原样保留
        assert_eq!(fit_window_size(1200.0, 700.0, 1536.0, 824.0), (1200.0, 700.0));
        // 超屏夹到 可用区−16（125% 脏配置 1750×1125 落在 1120×720 逻辑屏）
        assert_eq!(fit_window_size(1750.0, 1125.0, 1120.0, 720.0), (1104.0, 704.0));
        // 仅一个维度超
        assert_eq!(fit_window_size(1600.0, 650.0, 1536.0, 824.0), (1520.0, 650.0));
        // 极小屏不会夹出负/零尺寸
        let (w, h) = fit_window_size(1750.0, 1125.0, 300.0, 200.0);
        assert_eq!((w, h), (320.0, 240.0));
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 单实例：重复双击 exe 时不另起新进程（避免再冷启动一个 WebView2、窗口出现在后台），
        // 而是直接把已运行实例的主窗口唤起并置前
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            crate::tray::show_window(app);
        }))
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                // 插件默认时间戳取 now_utc（比本地慢 8 小时，曾让人把上午的日志读成凌晨）；
                // 自带 timezone_strategy(UseLocal) 会把布局改成 [级别][target]，
                // 这里复刻默认布局 [日期][时间][target][级别]，只换成本地时间
                .format(|out, message, record| {
                    let now = chrono::Local::now();
                    out.finish(format_args!(
                        "[{}][{}][{}][{}] {}",
                        now.format("%Y-%m-%d"),
                        now.format("%H:%M:%S"),
                        record.target(),
                        record.level(),
                        message
                    ))
                })
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Webview),
                    // 文件日志写到数据根目录（默认 app_log_dir 是 %LOCALAPPDATA%\logs，不符合统一目录约定）
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Folder {
                        path: crate::paths::data_root().join("logs"),
                        file_name: Some("x-hub".into()),
                    }),
                ])
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        // 笔记图片协议：笔记 Markdown 内嵌 http://xhub-note.localhost/<hash>.<ext>，
        // 按「数据根/notes/images/<文件名>」读取（URL 不含数据根绝对路径，迁数据目录后仍有效）；
        // 文件名严格校验为 16 位十六进制哈希 + 白名单扩展名，杜绝路径穿越
        .register_uri_scheme_protocol("xhub-note", |_ctx, request| {
            let name = request.uri().path().trim_start_matches('/');
            let valid = {
                let mut parts = name.split('.');
                match (parts.next(), parts.next(), parts.next()) {
                    (Some(hash), Some(ext), None) => {
                        hash.len() == 16
                            && hash.chars().all(|c| c.is_ascii_hexdigit())
                            && matches!(
                                ext.to_lowercase().as_str(),
                                "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif"
                            )
                    }
                    _ => false,
                }
            };
            if !valid {
                return tauri::http::Response::builder()
                    .status(400)
                    .body(Vec::new())
                    .unwrap();
            }
            let mime = match name.rsplit('.').next().unwrap_or("").to_lowercase().as_str() {
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "webp" => "image/webp",
                "bmp" => "image/bmp",
                "gif" => "image/gif",
                _ => "application/octet-stream",
            };
            match std::fs::read(crate::paths::data_root().join("notes").join("images").join(name))
            {
                Ok(bytes) => tauri::http::Response::builder()
                    .header("Content-Type", mime)
                    .header("Access-Control-Allow-Origin", "*")
                    // 内容哈希命名 ⇒ 同名即同内容，可长缓存
                    .header("Cache-Control", "public, max-age=31536000, immutable")
                    .body(bytes)
                    .unwrap(),
                Err(_) => tauri::http::Response::builder()
                    .status(404)
                    .body(Vec::new())
                    .unwrap(),
            }
        })
        // 扩展内容协议：扩展入口与其相对资源的唯一来源。
        // origin = `xhub-ext.localhost`，与承载用户数据的 `asset.localhost` 跨源，
        // 扩展因此无法直接读取数据根下的数据库 / 配置（见 docs/adr/0008）
        .register_uri_scheme_protocol("xhub-ext", |ctx, request| {
            crate::ext_protocol::handle(ctx.app_handle(), request)
        })
        .setup(|app| {
            log::info!("========== x-hub 启动 ==========");

            // 资产协议作用域：**只**放行必要的子目录（图标 / 壁纸 / 剪贴板图片 / 扩展），
            // 绝不放行整个数据根，也绝不写回 tauri.conf 的 `$APPDATA/**`。
            //
            // 理由（见 docs/adr/0008-extension-content-origin-isolation.md）：资产协议作用域
            // 是**全局单例**，而扩展 iframe 与资产资源同源 ⇒ 放行数据根等于任何扩展都能直接
            // fetch 到用户数据库（xhub.db）、app.json 与日志，从而绕开桥 API 的权限系统。
            // 数据目录可被改到 %APPDATA% 之外（自定义目录 / U 盘便携），故必须动态放行。
            const ASSET_SCOPE_SUBDIRS: [&str; 3] =
                ["icons", "wallpapers", "clipboard/images"];
            for rel in ASSET_SCOPE_SUBDIRS {
                let dir = crate::paths::data_root().join(rel);
                // 目录必须先存在：allow_directory 会额外注册 canonicalize 后的模式变体，
                // 目录不存在时拿不到该变体，请求侧 canonicalize 后就匹配不上（403）。
                if let Err(e) = std::fs::create_dir_all(&dir) {
                    log::warn!("资产作用域目录创建失败 {}: {e}", dir.display());
                }
                if let Err(e) = app.asset_protocol_scope().allow_directory(&dir, true) {
                    log::warn!("资产作用域放行失败 {}: {e}", dir.display());
                }
            }

            // 旧版本（com.workbench.desktop 标识）数据迁移到 x-hub 目录
            migrate_legacy_data();

            // 升级自替换非常早期执行：必须在数据库/其他句柄持有 exe 相关资源前，
            // 且仅在真正待应用时才做替换（幂等）。失败只记日志不阻断启动。
            updater::apply_pending_update(app.handle(), &app.package_info().version.to_string());

            let conn = init_database()?;
            fix_icon_paths(&conn);
            app.manage(DbState(std::sync::Mutex::new(conn)));
            app.manage(clipboard::ClipboardState::default());
            app.manage(service::ServiceState::default());
            // 开发扩展目录映射（「我的扩展」登记；扩展协议与扫描按它解析源码目录）
            app.manage(ext_protocol::DevExtensionDirs::default());
            // 本机源码目录重放：资产作用域放行不落盘，重启必须按配置重新放行
            extension::apply_dev_extensions(app.handle());

            // 速记改造启动维护（幂等、轻量）：按保留天数清回收站 + 存量笔记补建双链索引。
            // 延后 15s：避开启动期 DB 锁争抢（与图标清扫同节奏）。
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                    let state = handle.state::<DbState>();
                    let lock = state.0.lock();
                    let Ok(conn) = lock else {
                        return;
                    };
                    let days = config::load().note_trash_retention_days;
                    match crate::repo::note::purge_expired(&conn, days) {
                        Ok(n) if n > 0 => log::info!("回收站启动清理: {n} 条（保留 {days} 天）"),
                        Ok(_) => {}
                        Err(e) => log::warn!("回收站启动清理失败: {e}"),
                    }
                    if let Err(e) = crate::repo::note_link::reindex_all(&conn) {
                        log::warn!("笔记双链索引重建失败: {e}");
                    }
                });
            }

            // 账号会话启动校验（异步，不阻塞窗口创建）：token 失效则静默清理
            let account_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                account::verify_session_on_startup(&account_handle).await;
            });

            // 启动扩展反向代理（/svc/<extId>/* → 127.0.0.1:<service 端口>，统一加 CORS 头）
            let proxy_port = tauri::async_runtime::block_on(proxy::start(app.handle().clone()))
                .unwrap_or_else(|e| {
                    log::warn!("扩展反向代理启动失败: {e}");
                    0
                });
            app.manage(proxy::ProxyState::new(proxy_port));

            tray::setup(app)?;
            shortcut::setup(app)?;

            restore_window_state(app);

            // 自启动静默模式（--autostart-hidden）：主窗不显示、直接驻留托盘。
            // 必须经 tray::hide_window 更新自维护的显隐状态位，否则 MAIN_WINDOW_VISIBLE
            // 保持默认 true，全局快捷键 toggle_window 会误判「窗口可见」，只 set_focus
            // 隐藏窗口上无效调用，导致快捷键无法呼出主窗口。
            if crate::autostart::is_hidden_launch() {
                crate::tray::hide_window(app.handle());
            }

            // 桌面悬浮球（ADR 0004）：须在 autostart-hidden 的 hide_window 之后初始化，
            // 显隐联动（主窗隐藏 → 球显示）才能覆盖静默启动场景
            floating_ball::init(app.handle());

            // 主窗口启动时隐藏（tauri.conf.json visible:false），等前端内容可绘制后再 show，
            // 避免 WebView2 冷启动期间出现空白/白屏等待窗口；这里先铺上主题底色，
            // 若 show 早于首帧绘制，也只会闪主题色而非纯白
            // 主题三态：dark，或 system 且系统偏好深色 → 暗色底色；light / 系统浅色 → 亮色
            let config = config::load();
            // 开机自启动自愈：用户开了自启动、但 Run 键因换目录/被清理工具删除/路径变更而失效时，
            // 按当前 exe 重写。放在主窗显示前，静默修复，不阻断启动。
            if config.run_at_startup {
                match autostart::ensure_registered() {
                    // 自愈详情已由 ensure_registered 内部记录（键缺失/路径不符两种），
                    // 这里不再重复写一条
                    Ok(true) => {}
                    Ok(false) => {}
                    Err(e) => log::warn!("开机自启动自愈失败: {e}"),
                }
            }
            let mut dark = config.theme_mode == "dark";
            if config.theme_mode == "system" {
                dark = matches!(
                    main_window(app.handle()).and_then(|w| w.theme().ok()),
                    Some(tauri::Theme::Dark)
                );
            }
            let bg = if dark {
                tauri::window::Color(18, 19, 27, 255) // --bg-page 暗色 #12131b
            } else {
                tauri::window::Color(236, 239, 246, 255) // --bg-page 亮色 #eceff6
            };
            if let Some(window) = main_window(app.handle()) {
                let _ = window.set_background_color(Some(bg));
                // WebviewWindow::set_background_color 原本同时铺窗口与 webview 两侧底色，
                // Window 版只铺窗口侧，这里补 webview 侧保持原行为（get_webview 按
                // label 查 webview 表，多 webview 窗口不受影响）
                if let Some(webview) = app.get_webview("main") {
                    let _ = webview.set_background_color(Some(bg));
                }
                // 启动即前台：避免窗口偶尔出现在其他窗口后面；稍等再补一次焦点，
                // 绕开 Windows 前台锁定的瞬时限制
                let _ = window.set_focus();
                let handle = window.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    let _ = handle.set_focus();
                });
            }

            // 恢复上次已脱离的浮窗便签（位置/置顶/内容均持久化）
            {
                let state = app.state::<DbState>();
                let conn = state.0.lock().map_err(|e| e.to_string())?;
                let detached = crate::repo::detached_sticky::list(&conn).unwrap_or_default();
                drop(conn);
                sticky_window::restore_all(app.handle(), &detached);
            }

            // 恢复上次已浮起的倒计时浮窗（浮起状态 + 位置持久化）
            {
                let state = app.state::<DbState>();
                let conn = state.0.lock().map_err(|e| e.to_string())?;
                let floated = crate::repo::countdown::list_floated(&conn).unwrap_or_default();
                drop(conn);
                countdown_window::restore_all(app.handle(), &floated);
            }

            // 工作台倒计时卡片可见性门控：默认不可见，待前端按已提交布局上报后放开，
            // 避免启动早期（前端未就绪/卡片实际不在布局中）倒计时抢跑到点
            app.manage(countdown_ticker::CardVisible(
                std::sync::atomic::AtomicBool::new(false),
            ));

            // 启动倒计时后台驱动线程（每秒扫描到期项，托盘/隐藏时不受 WebView 节流影响）
            countdown_ticker::start(app.handle().clone());

            // 启动待办提醒后台线程（remind_at 到点发系统通知 + 前端 toast）
            todo_reminder::start(app.handle().clone());

            // 启动剪贴板监听线程（启动零加载历史，仅剪贴板变化时落库）
            clipboard::start_monitor(app.handle().clone());

            // 启动剪贴板延迟窗口操作 worker（粘贴/归还焦点统一串行执行，避免频繁 spawn 短命线程）
            clipboard::init_win_op_worker();

            // 预创建剪贴板浮层窗口（隐藏常驻）：运行时现场创建 WebView2 窗口曾与
            // 悬浮球操作交错导致整窗未响应（见 clipboard.rs::init_overlay_window 注释）
            clipboard::init_overlay_window(app.handle());

            // 预创建通知窗（隐藏常驻）：右下角自绘通知，跨 Win10/11 一致（详见 notify.rs）
            notify::init(app.handle());

            // AI 对话独立窗口（无条件预创建隐藏常驻，运行期绝不建窗，详见 chat_window.rs）
            chat_window::init(app.handle());

            // 速达「应用内打开网页」：主窗内嵌面板 webview + 独立浏览器窗口池×1
            // （全部页面收进 tab；无条件预创建隐藏常驻，运行期只 show/hide/navigate/set_bounds，见 suda_browser.rs）
            suda_browser::init(app.handle());

            // WebView2 内存级别联动（P0，webview_mem.rs）：窗口隐藏时把常驻 renderer
            // 的内存目标级别设为 Low（弃缓存换页、脚本照常跑），显示前恢复 Normal。
            // 快路径接在各显隐函数里，这里起的是 300ms 轮询纠偏兜底线程
            webview_mem::init(app.handle());

            // 关闭事件：拦截默认关闭，改为隐藏至托盘
            if let Some(window) = main_window(app.handle()) {
                let app_handle = app.handle().clone();
                let win_for_events = window.clone();
                window.on_window_event(move |event| {
                    match event {
                        tauri::WindowEvent::CloseRequested { api, .. } => {
                            log::info!("收到关闭请求：保存状态并隐藏至托盘");
                            persist_window_state(&app_handle);
                            api.prevent_close();
                            crate::tray::hide_window(&app_handle);
                        }
                        // 最小化/还原联动悬浮球：最小化也是主窗「视觉不可见」，
                        // 球应出现。Windows 上最小化状态变化伴随 Resized 事件，借此
                        // 检测（MAIN_WINDOW_VISIBLE 状态位只覆盖托盘/快捷键显隐链）
                        tauri::WindowEvent::Resized(_) => {
                            let minimized = win_for_events.is_minimized().unwrap_or(false);
                            crate::floating_ball::set_main_minimized(&app_handle, minimized);
                        }
                        _ => {}
                    }
                });
            }

            // 全局快捷键事件：切换主窗口显示/隐藏
            let app_handle = app.handle().clone();
            app.listen("global-shortcut-toggle", move |_| {
                crate::tray::toggle_window(&app_handle);
            });

            // 剪贴板快捷键事件：唤起/收起剪贴板历史浮层
            let app_handle = app.handle().clone();
            app.listen("clipboard-toggle", move |_| {
                crate::clipboard::toggle_overlay(&app_handle);
            });

            // 搜索快捷键事件：唤起主窗（对话框由主窗前端收到同名事件后打开）
            let app_handle = app.handle().clone();
            app.listen("search-shortcut", move |_| {
                crate::tray::show_window(&app_handle);
            });

            // 速记快捷键事件：唤起主窗（切速记视图 + 聚焦新建由主窗前端收到同名事件后做）
            let app_handle = app.handle().clone();
            app.listen("notes-shortcut", move |_| {
                crate::tray::show_window(&app_handle);
            });

            // AI 对话快捷键事件：抽屉形态先唤起主窗（面板在主窗里）；
            // 独立窗口形态不弹主窗，由主窗前端收到事件后直接唤起对话小窗
            let app_handle = app.handle().clone();
            app.listen("chat-shortcut", move |_| {
                if !crate::chat_window::mode_enabled() {
                    crate::tray::show_window(&app_handle);
                }
            });

            // 静默检查更新：启动 5s 后一次，此后按配置间隔（默认 4h）循环。
            // 受 auto_update_enabled 开关控制；检查失败静默（updater 内部记日志）。
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    loop {
                        {
                            let cfg = crate::config::load();
                            if cfg.auto_update_enabled {
                                let _ = updater::check_for_update(handle.clone(), None).await;
                            }
                        }
                        let hours = crate::config::load().update_interval_hours.max(1);
                        tokio::time::sleep(std::time::Duration::from_secs(hours * 3600)).await;
                    }
                });
            }

            // 低清图标缓存升级：启动 15s 后一次性后台清扫（逐个串行，IO 前释放 DB 锁）。
            // 旧 PowerShell 链路只产出 32×32 缓存，高分屏发糊；此处就地重提为 256×256，
            // 图标路径不变（键 = target 哈希），前端下次挂载/重启即见高清图。
            {
                let handle = app.handle().clone();
                std::thread::Builder::new()
                    .name("icon-sweep".into())
                    .spawn(move || {
                        std::thread::sleep(std::time::Duration::from_secs(15));
                        commands::sweep_stale_icons(&handle);
                    })
                    .ok();
            }

            log::info!("x-hub 启动完成");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_initial_data,
            commands::create_resource,
            commands::update_resource,
            commands::get_resource_remark,
            commands::delete_resource,
            commands::reorder_resources,
            commands::launch_resource,
            commands::launch_resource_as_admin,
            commands::reveal_resource_in_explorer,
            commands::list_installed_browsers,
            commands::open_url_with_browser,
            commands::create_note,
            commands::create_note_in,
            commands::update_note,
            commands::delete_note,
            commands::list_notes,
            commands::get_note,
            // 速记改造：回收站 / 文件夹树 / 图片 GC / 双链（docs/speednote-plan.md）
            commands::trash_note,
            commands::restore_note,
            commands::purge_note,
            commands::purge_expired_notes,
            commands::list_trashed_notes,
            commands::list_note_folders,
            commands::create_note_folder,
            commands::rename_note_folder,
            commands::delete_note_folder,
            commands::reorder_note_folders,
            commands::set_note_folder,
            commands::set_note_icon,
            commands::purge_all_trashed_notes,
            commands::gc_orphan_note_images,
            commands::get_note_links,
            commands::rebuild_note_links,
            // 速记导出/导入（docs/speednote-plan.md §8，限定自有产物）
            note_io::export_notes,
            note_io::import_notes,
            note_io::import_notes_cancel,
            commands::list_todos,
            commands::create_todo,
            commands::toggle_todo,
            commands::update_todo,
            commands::delete_todo,
            commands::schedule_todo,
            commands::reorder_todo_orders,
            commands::move_todo_child,
            commands::set_todo_description,
            commands::set_todo_pinned,
            commands::set_todo_repeat,
            commands::complete_todo_recurring,
            commands::undo_todo_recurring,
            commands::expand_todo_occurrences,
            commands::list_todo_tags,
            commands::create_todo_tag,
            commands::update_todo_tag,
            commands::delete_todo_tag,
            commands::set_todo_tags,
            commands::list_todo_tag_links,
            commands::list_stickies,
            commands::save_sticky,
            commands::get_detached_stickies,
            commands::detach_sticky,
            commands::focus_detached_sticky,
            commands::save_detached_sticky,
            commands::toggle_detached_sticky_pin,
            commands::restore_detached_sticky,
            commands::delete_detached_sticky,
            commands::list_countdowns,
            commands::create_countdown,
            commands::update_countdown,
            commands::delete_countdown,
            commands::pause_countdown,
            commands::resume_countdown,
            commands::float_countdown,
            commands::unfloat_countdown,
            commands::set_countdown_card_visible,
            commands::list_snippets,
            commands::create_snippet,
            commands::update_snippet,
            commands::delete_snippet,
            commands::toggle_snippet_pin,
            commands::record_snippet_copy,
            commands::toggle_prompt_float,
            commands::toggle_todo_float,
            commands::toggle_float_pin,
            commands::search_all,
            commands::save_config,
            commands::set_window_always_on_top,
            commands::set_always_on_top_config,
            commands::get_global_shortcut,
            commands::set_global_shortcut,
            commands::set_search_shortcut,
            commands::set_chat_shortcut,
            commands::set_notes_shortcut,
            commands::set_shortcut_enabled,
            commands::get_run_at_startup,
            commands::set_run_at_startup,
            commands::get_startup_hidden,
            notify::notice_ready,
            notify::notice_layout,
            notify::notice_dismiss_window,
            commands::log_client_error,
            commands::minimize_window,
            commands::toggle_maximize,
            commands::hide_to_tray,
            commands::parse_dropped_path,
            commands::import_icon_file,
            commands::import_wallpaper,
            commands::cleanup_wallpapers,
            commands::import_note_image,
            commands::inspect_path,
            commands::scan_installed_apps,
            commands::scan_desktop,
            commands::delete_desktop_shortcuts,
            commands::scan_browser_bookmarks,
            commands::fetch_favicons,
            commands::get_running_processes,
            commands::list_tags,
            commands::create_tag,
            commands::rename_tag,
            commands::delete_tag,
            commands::get_note_tags,
            commands::set_note_tags,
            commands::list_note_tags,
            commands::backup_data,
            commands::restore_data,
            commands::get_data_path,
            commands::change_data_dir,
            commands::restart_app,
            commands::list_chat_sessions,
            commands::create_chat_session,
            commands::delete_chat_session,
            commands::rename_chat_session,
            commands::set_chat_session_model,
            commands::list_chat_messages,
            commands::send_chat_message,
            commands::ai_transform_note,
            commands::get_chat_models,
            commands::save_chat_models,
            commands::fetch_chat_provider_models,
            commands::get_chat_api_key,
            commands::set_chat_panel,
            commands::get_chat_panel,
            commands::set_chat_panel_side,
            commands::get_ui_config,
            chat_window::chat_window_get_state,
            chat_window::chat_window_toggle,
            chat_window::chat_window_close,
            chat_window::chat_window_set_pinned,
            chat_window::chat_window_save_mode,
            chat_window::chat_window_open_settings,
            suda_browser::suda_browser_open,
            suda_browser::suda_browser_open_url,
            suda_browser::suda_browser_open_tab,
            suda_browser::suda_browser_activate_tab,
            suda_browser::suda_browser_close_tab,
            suda_browser::suda_browser_close,
            suda_browser::suda_browser_navigate,
            suda_browser::suda_browser_back,
            suda_browser::suda_browser_forward,
            suda_browser::suda_browser_reload,
            suda_browser::suda_browser_open_system,
            suda_browser::suda_browser_chrome_height,
            suda_browser::suda_browser_state,
            suda_browser::suda_browser_slots,
            suda_browser::suda_panel_show,
            suda_browser::suda_panel_bounds,
            suda_browser::suda_panel_hide,
            suda_browser::suda_panel_navigate,
            suda_browser::suda_panel_back,
            suda_browser::suda_panel_forward,
            suda_browser::suda_panel_reload,
            commands::list_subcategories,
            commands::create_subcategory,
            commands::rename_subcategory,
            commands::delete_subcategory,
            commands::reorder_subcategories,
            commands::set_default_subcategory,
            commands::list_zones,
            commands::create_zone,
            commands::rename_zone,
            commands::delete_zone,
            commands::reorder_zones,
            commands::resize_zone,
            commands::set_resources_zone,
            commands::reorder_resources_zoned,
            commands::set_suda_web_open_mode,
            commands::get_app_info,
            commands::clipboard_list,
            commands::clipboard_copy,
            commands::clipboard_paste,
            commands::clipboard_toggle_pin,
            commands::clipboard_delete,
            commands::clipboard_clear,
            commands::clipboard_set_paused,
            commands::clipboard_activate,
            commands::clipboard_hide,
            commands::set_clipboard_paste_method,
            commands::set_clipboard_media_enabled,
            commands::clipboard_export_image,
            commands::clipboard_get_info,
            commands::set_clipboard_shortcut,
            commands::set_clipboard_retention,
            sysmon::get_system_info,
            extension::list_extensions,
            extension::extensions_stamp,
            extension::read_extension_entry,
            extension::open_extension_window,
            extension::open_extension_dir,
            extension::uninstall_extension,
            extension::get_extension_permissions,
            extension::set_extension_permission,
            extension::get_dev_mode_status,
            extension::add_dev_extension,
            extension::remove_dev_extension,
            extension::dev_extensions_stamp,
            // 扩展开发技能包（Skills）：内置 x-hub-extension 一键装到本机 AI 助手的 skills 目录
            skills::get_skill_overview,
            skills::install_skill,
            skills::uninstall_skill,
            skills::remove_skill_root,
            // 平台账号（登录 / 额度 / 开发者申请）
            // 注意：服务端地址内置为常量（见 config::DEFAULT_SERVER_URL），无 account_set_server 命令
            account::account_status,
            account::account_login_github_start,
            account::account_login_github_poll,
            account::account_login_email_send,
            account::account_login_email_verify,
            account::account_logout,
            account::account_redeem,
            account::dev_apply,
            account::dev_apply_status,
            account::account_list_devices,
            account::account_revoke_device,
            // 扩展发布（打包上传 / 我的提交 / 撤回）
            publisher::dev_submit,
            // 发布弹窗的截图缩略图预览（读本地图为 data URL）
            publisher::read_image_data_url,
            // 发布弹窗「引用上一版截图」：下载市场清单里已上架版本的截图到临时文件
            publisher::fetch_remote_screenshots,
            publisher::dev_list_submissions,
            publisher::dev_get_submission,
            publisher::dev_withdraw_submission,
            // 发布前本地预检（作者侧 lint：只做已开源口径的检查，不作为放行依据）
            precheck::precheck_extension,
            // 平台 AI 额度（登录后可直接使用的模型列表）
            chat::platform_models,
            market::get_market_registry,
            market::refresh_market_registry,
            market::install_from_market,
            market::install_local_archive,
            market::pack_extension_archive,
            market::update_extension,
            updater::check_for_update,
            updater::download_update,
            updater::get_update_status,
            updater::skip_update_version,
            updater::snooze_update,
            process::open_external,
            xhub_api::xhub_call,
            commands::check_connectivity,
            commands::get_weather,
            commands::get_quote,
            commands::set_weather_city,
            commands::locate_weather_by_ip,
            floating_ball::floating_ball_get_state,
            floating_ball::floating_ball_save_settings,
            floating_ball::floating_ball_drag_begin,
            floating_ball::floating_ball_drag_cancel,
            floating_ball::floating_ball_expand,
            floating_ball::floating_ball_trigger,
            floating_ball::floating_ball_context_menu,
            floating_ball::floating_ball_reapply,
            commands::get_theme_config,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // 宿主退出：停止所有 service 后端进程，避免 Node 子进程残留
            if let tauri::RunEvent::Exit = event {
                service::stop_all(app);
                // 先显式移除托盘图标：set_visible/destroy 那类 API 会 run_on_main_thread 回投到
                // 事件循环（本回调正在主线程里跑，投回去没人处理，就是当年「退出流程被拖死」的
                // 真凶）；remove_tray_by_id 是同步移除 + 析构发 Shell_NotifyIcon(NIM_DELETE)，
                // 不依赖事件循环，既不解架也消掉 Explorer 里悬停才清的幽灵图标。
                app.remove_tray_by_id("main-tray");
                // Windows 上 WebView2 子窗口销毁偶发把退出流程拖死（托盘点「退出」
                // 后进程不消失），清理完成后直接结束进程，保证退出 100% 生效
                std::process::exit(0);
            }
        });
}
