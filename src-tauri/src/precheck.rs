//! 发布前本地预检（作者侧 lint）。
//!
//! 定位：**只做已开源口径的检查**（manifest schema + 权限申报 + 能力可用性），让作者在点「发布」
//! 之前就发现问题，而不是上传后被关卡挡回来再改一轮。它**不复制服务端的审核规则**，
//! 也**不构成放行依据**（PRD 附录 A 红线：客户端只问不判）。
//!
//! 与 `x-hub-extensions/scripts/validate.mjs` 同一套思路，只是搬到客户端里自动跑。

use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct PrecheckItem {
    /// ok / warn / error
    pub level: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrecheckResult {
    pub items: Vec<PrecheckItem>,
    /// 是否没有 error 级问题（warn 不阻止发布）
    pub clean: bool,
}

fn err(label: &str, detail: String) -> PrecheckItem {
    PrecheckItem {
        level: "error".into(),
        label: label.into(),
        detail: Some(detail),
    }
}

fn ok(label: &str) -> PrecheckItem {
    PrecheckItem {
        level: "ok".into(),
        label: label.into(),
        detail: None,
    }
}

fn warn(label: &str, detail: String) -> PrecheckItem {
    PrecheckItem {
        level: "warn".into(),
        label: label.into(),
        detail: Some(detail),
    }
}

fn is_semver(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

fn id_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && !id.starts_with('.')
        && !id.contains("..")
        && id.contains('.')
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
}

/// `window.xhub` 上的**顶层方法**（JS 调用链只有一段）→ 运行时能力名（dispatch 的 namespace.method）。
///
/// 为什么需要这张表：`scan_bridge_calls` 只收「命名空间.方法」形态的链，而顶层方法的链只有一段
/// （`window.xhub.openExternal(...)`），于是它**完全消失**在「能力是否实现 / 权限是否申报」两项对账之外——
/// 运行时从 v0.6.6 起要求 `openExternal` 声明权限，预检却一直判"不需要"，正是这条漏检造成的。
const TOP_LEVEL_METHODS: &[(&str, &str)] = &[
    ("openExternal", "runtime.openExternal"),
    ("openPermissions", "runtime.openPermissions"),
];

/// 从 `at` 起跳过空白后是否紧跟 `(`（即这是一次**调用**，不是属性访问）。
fn is_call_at(bytes: &[u8], at: usize) -> bool {
    let mut k = at;
    while k < bytes.len() && (bytes[k] as char).is_whitespace() {
        k += 1;
    }
    k < bytes.len() && bytes[k] == b'('
}

/// 扫描源码里的桥 API 调用，返回**完整标识符链**（如 `data.notes.create`）；
/// 顶层方法换算成运行时能力名（如 `openExternal` → `runtime.openExternal`）。
///
/// 必须取整条链：`xhub.data.notes.create(...)` 与 `xhub.data.notes.list(...)` 的命名空间相同，
/// 只有**最后一段**才是方法名 —— 只取两段会把"写"误判成"读"（这个 bug 曾在预检与服务端关卡里同时存在）。
/// 手写扫描以避免为一个 lint 引入正则依赖。
fn scan_bridge_calls(source: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let bytes = source.as_bytes();
    let mut i = 0usize;
    while let Some(pos) = source[i..].find("xhub.") {
        let start = i + pos + 5;
        let mut j = start;
        loop {
            let seg_start = j;
            while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                j += 1;
            }
            if j == seg_start {
                break;
            }
            // 点号后面紧跟标识符才继续（`xhub.data.` 收尾不算链）
            if j < bytes.len()
                && bytes[j] == b'.'
                && j + 1 < bytes.len()
                && (bytes[j + 1].is_ascii_alphanumeric() || bytes[j + 1] == b'_')
            {
                j += 1;
                continue;
            }
            break;
        }
        if j > start {
            let chain = &source[start..j];
            if chain.contains('.') {
                // 「命名空间.方法」形态：照收
                out.push(chain.to_string());
            } else if is_call_at(bytes, j) {
                // 单段链：`window.xhub.storage` 这类属性访问（后面没有 `(`）不是调用，跳过；
                // 而 `xhub.openExternal(...)` 是真调用，换算成能力名收进来（见 TOP_LEVEL_METHODS）
                if let Some((_, cap)) = TOP_LEVEL_METHODS.iter().find(|(m, _)| *m == chain) {
                    out.push((*cap).to_string());
                }
            }
        }
        i = j.max(i + 1);
    }
    out.sort();
    out.dedup();
    out
}

/// 从完整链里取 `(命名空间, 方法名)`：`data.notes.create` → `("data", "create")`
fn split_chain(chain: &str) -> Option<(&str, &str)> {
    let mut parts = chain.split('.');
    let ns = parts.next()?;
    let last = chain.rsplit('.').next()?;
    if ns.is_empty() || last.is_empty() || ns == last {
        return None;
    }
    Some((ns, last))
}

/// 命名空间 → 所需权限（与服务端关卡同口径；两处必须一致，否则会"本地过了线上被拒"）
fn permission_for(ns: &str, method: &str) -> Option<&'static str> {
    const WRITE_PREFIXES: [&str; 8] = [
        "create", "update", "delete", "set", "toggle", "reorder", "import", "schedule",
    ];
    // `openExternal` 是**顶层方法**（`window.xhub.openExternal(...)`，不是 `runtime.openExternal`），
    // 由 `scan_bridge_calls` 按 `TOP_LEVEL_METHODS` 换算成能力名 `runtime.openExternal` 后落到这里。
    // v0.6.6 把运行时改成需要权限却没同步这里与 gate.ts，于是按文档写的扩展预检能过、装上却点不动链接。
    if ns == "runtime" && method == "openExternal" {
        return Some("open-url");
    }
    match ns {
        "data" => Some(if WRITE_PREFIXES.iter().any(|p| method.starts_with(p)) {
            "data:write"
        } else {
            "data:read"
        }),
        "ui" => Some("notify"),
        "net" => Some("network"),
        "system" => Some("system"),
        "clipboard" => Some("clipboard"),
        "fs" => Some("fs"),
        "sharedStorage" => Some("shared-storage"),
        "events" => {
            if method == "emit" {
                Some("events")
            } else {
                None
            }
        }
        _ => None, // runtime / storage / config / theme / service / expose 不需要权限
    }
}

fn collect_sources(dir: &Path, out: &mut Vec<(String, String)>, depth: u32) {
    if depth > 6 || out.len() >= 200 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        if path.is_dir() {
            collect_sources(&path, out, depth + 1);
        } else {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !matches!(ext.as_str(), "html" | "htm" | "js" | "mjs" | "cjs") {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                out.push((name, text));
            }
        }
    }
}

/// 跑一遍本地预检
pub fn precheck(dir: &Path) -> PrecheckResult {
    let mut items: Vec<PrecheckItem> = Vec::new();

    let manifest = match crate::extension::read_manifest(dir) {
        Ok(m) => {
            items.push(ok("manifest.json 可解析"));
            m
        }
        Err(e) => {
            items.push(err("manifest.json 解析失败", e));
            return PrecheckResult {
                clean: false,
                items,
            };
        }
    };

    if !id_ok(&manifest.id) {
        items.push(err(
            "扩展 id 不合法",
            format!("{}（应为小写反向域名，如 com.example.myext）", manifest.id),
        ));
    } else if manifest.id.starts_with("com.x-hub.") {
        // 平台保留空间：本地预检只提示、不阻断——平台方自己发扩展也要用它，
        // 而「你是不是官方」只有服务端知道（关卡按提交者身份放行官方账号）。客户端「只问不判」。
        items.push(warn(
            "扩展 id 使用了平台保留命名空间",
            "com.x-hub.* 保留给官方扩展；非官方账号提交会被服务端拒绝，请换成你自己的反向域名（如 com.example.xxx）".into(),
        ));
    } else {
        items.push(ok(&format!("扩展 id：{}", manifest.id)));
    }

    if !is_semver(&manifest.version) {
        items.push(err("版本号不是 x.y.z 形式", manifest.version.clone()));
    } else {
        items.push(ok(&format!("版本号：{}", manifest.version)));
    }

    // 入口文件必须存在
    let mut missing: Vec<String> = Vec::new();
    for (surface, rel) in &manifest.entry {
        let p = dir.join(rel.trim_start_matches("./"));
        if !p.is_file() {
            missing.push(format!("{surface} → {rel}"));
        }
    }
    if manifest.entry.is_empty() {
        items.push(warn("没有声明任何入口（entry）", "至少需要一个形态的入口 HTML".into()));
    } else if missing.is_empty() {
        items.push(ok("入口文件齐全"));
    } else {
        items.push(err("入口文件不存在", missing.join("、")));
    }

    // service 扩展的后端声明
    if manifest.runtime == crate::extension::ExtensionRuntime::Service {
        match manifest.backend.as_ref() {
            None => items.push(err("service 扩展缺少 backend 声明", "manifest 里需要 backend.entry".into())),
            Some(b) => {
                let p = dir.join(b.entry.trim_start_matches("./"));
                if p.is_file() {
                    items.push(ok("service 后端入口存在"));
                } else {
                    items.push(err("service 后端入口不存在", b.entry.clone()));
                }
                if b.is_external() && !manifest.permissions.iter().any(|x| x == "network") {
                    items.push(err(
                        "对外监听却没有申请 network 权限",
                        "backend.host 指向非回环地址时必须声明 network".into(),
                    ));
                }
            }
        }
    }

    // 桥 API 调用：能力是否已实现 + 权限是否申报
    let mut sources: Vec<(String, String)> = Vec::new();
    collect_sources(dir, &mut sources, 0);
    let mut calls: Vec<String> = Vec::new();
    for (_, text) in &sources {
        calls.extend(scan_bridge_calls(text));
    }
    calls.sort();
    calls.dedup();

    let caps = crate::extension::host_capabilities();
    let missing_caps: Vec<String> = calls.iter().filter(|c| !caps.contains(*c)).cloned().collect();
    if missing_caps.is_empty() {
        items.push(ok("用到的桥 API 宿主都已实现"));
    } else {
        items.push(warn(
            "用到了宿主尚未实现的桥 API",
            format!("{}（换个实现方式，或等宿主更新）", missing_caps.join("、")),
        ));
    }

    let mut required: Vec<String> = Vec::new();
    for c in &calls {
        let Some((ns, method)) = split_chain(c) else {
            continue;
        };
        if let Some(p) = permission_for(ns, method) {
            if !required.contains(&p.to_string()) {
                required.push(p.to_string());
            }
        }
    }
    let undeclared: Vec<String> = required
        .iter()
        .filter(|p| !manifest.permissions.contains(*p))
        .cloned()
        .collect();
    if undeclared.is_empty() {
        items.push(ok("权限申报与实际调用一致"));
    } else {
        items.push(err(
            "代码用到了未在 manifest 里申请的权限",
            format!("{}（请加到 manifest.permissions）", undeclared.join("、")),
        ));
    }

    let unused: Vec<String> = manifest
        .permissions
        .iter()
        .filter(|p| p.as_str() != "network" && !required.contains(*p))
        .cloned()
        .collect();
    if !unused.is_empty() {
        items.push(warn(
            "manifest 声明了但代码里未见调用的权限",
            format!("{}（用不到就删掉，审核时更好过）", unused.join("、"))),
        );
    }

    let clean = !items.iter().any(|i| i.level == "error");
    PrecheckResult { items, clean }
}

/// 发布前本地预检（Tauri 命令）：让作者在点发布之前就发现问题
#[tauri::command]
pub fn precheck_extension(app: tauri::AppHandle, id: String) -> Result<PrecheckResult, String> {
    let dir = crate::ext_protocol::resolve_ext_dir(&app, &id)?;
    let result = precheck(&dir);
    let errors = result.items.iter().filter(|i| i.level == "error").count();
    log::info!(
        "本地预检 {id}: {} 项，其中 {} 项需要修",
        result.items.len(),
        errors
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_bridge_calls() {
        let src = r#"window.xhub.data.notes.list(); xhub.storage.set("k", 1); xhub.net.fetch("x");"#;
        assert_eq!(
            scan_bridge_calls(src),
            vec!["data.notes.list", "net.fetch", "storage.set"]
        );
        // 不是调用（没有点号链）不应误报
        assert!(scan_bridge_calls("const xhub = 1;").is_empty());
    }

    #[test]
    fn splits_chain_into_namespace_and_method() {
        assert_eq!(split_chain("data.notes.list"), Some(("data", "list")));
        assert_eq!(split_chain("data.notes.create"), Some(("data", "create")));
        assert_eq!(split_chain("fs.saveText"), Some(("fs", "saveText")));
        assert_eq!(split_chain("data"), None);
    }

    #[test]
    fn scans_top_level_open_external_as_capability() {
        // 顶层方法（链只有一段）也必须进对账：openExternal → runtime.openExternal
        assert_eq!(
            scan_bridge_calls("window.xhub.openExternal('https://example.com')"),
            vec!["runtime.openExternal".to_string()]
        );
        // 属性访问不是调用，仍不收（否则会被误报成「用到了宿主尚未实现的桥 API」）
        assert!(scan_bridge_calls("const s = window.xhub.storage;").is_empty());
        // 「命名空间.方法」形态不受影响
        assert_eq!(
            scan_bridge_calls("window.xhub.data.notes.list()"),
            vec!["data.notes.list".to_string()]
        );
    }

    #[test]
    fn precheck_flags_missing_open_url_permission() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(
            dir.join("manifest.json"),
            r#"{"id":"com.example.link","name":"L","version":"1.0.0","entry":{"view":"./index.html"},"permissions":[]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("index.html"),
            "<script>window.xhub.openExternal('https://example.com')</script>",
        )
        .unwrap();

        let r = precheck(dir);
        assert!(
            !r.clean,
            "用了 openExternal 却没申报 open-url 应当判为需要修：{:?}",
            r.items
        );
    }

    #[test]
    fn maps_namespaces_to_permissions() {
        // 方法名取链的最后一段：读 vs 写必须区分得开
        assert_eq!(permission_for("data", "list"), Some("data:read"));
        assert_eq!(permission_for("data", "create"), Some("data:write"));
        assert_eq!(permission_for("data", "toggle"), Some("data:write"));
        assert_eq!(permission_for("fs", "saveText"), Some("fs"));
        assert_eq!(permission_for("events", "emit"), Some("events"));
        assert_eq!(permission_for("events", "on"), None);
        assert_eq!(permission_for("storage", "set"), None);
        // runtime 按方法区分：只有 openExternal 需要权限，其余 runtime.* 不需要
        assert_eq!(permission_for("runtime", "openExternal"), Some("open-url"));
        assert_eq!(permission_for("runtime", "info"), None);
        assert_eq!(permission_for("runtime", "open"), None);
    }

    #[test]
    fn precheck_flags_write_permission_for_create_call() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(
            dir.join("manifest.json"),
            r#"{"id":"com.example.w","name":"W","version":"1.0.0","entry":{"view":"./index.html"},"permissions":["data:read"]}"#,
        )
        .unwrap();
        // 只申报了 data:read，但代码在写 → 必须报错（正是「只取两段」会漏掉的场景）
        std::fs::write(
            dir.join("index.html"),
            "<script>window.xhub.data.notes.create({title:'x'})</script>",
        )
        .unwrap();

        let r = precheck(dir);
        assert!(!r.clean, "写了数据却只申报 data:read 应当判为需要修：{:?}", r.items);
    }

    #[test]
    fn validates_semver_and_id() {
        assert!(is_semver("1.0.0"));
        assert!(!is_semver("1.0"));
        assert!(!is_semver("v1.0.0"));
        assert!(id_ok("com.example.hello"));
        assert!(!id_ok("hello")); // 必须含点
        assert!(!id_ok("Com.Example"));
        assert!(!id_ok(".hidden.id"));
    }

    #[test]
    fn precheck_reports_missing_permission() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(
            dir.join("manifest.json"),
            r#"{"id":"com.example.x","name":"X","version":"1.0.0","entry":{"view":"./index.html"},"permissions":[]}"#,
        )
        .unwrap();
        std::fs::write(dir.join("index.html"), "<script>window.xhub.data.notes.list()</script>").unwrap();

        let r = precheck(dir);
        assert!(!r.clean, "缺少 data:read 申报应当判为需要修");
        assert!(r
            .items
            .iter()
            .any(|i| i.level == "error" && i.label.contains("未在 manifest 里申请")));
    }

    #[test]
    fn precheck_passes_for_clean_extension() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(
            dir.join("manifest.json"),
            r#"{"id":"com.example.ok","name":"OK","version":"0.1.0","entry":{"view":"./index.html"},"permissions":["data:read"]}"#,
        )
        .unwrap();
        std::fs::write(dir.join("index.html"), "<script>window.xhub.data.notes.list()</script>").unwrap();

        let r = precheck(dir);
        assert!(r.clean, "合规扩展不应有 error：{:?}", r.items);
    }
}
