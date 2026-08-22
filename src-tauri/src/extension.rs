//! 扩展系统：manifest 解析 + 本地目录扫描注册表（spec §12 第 1 步）。
//!
//! 第 1 步只做「扫描 + 解析 + 列出」，不涉及安装 / 卸载 / 打开 / service 进程托管。
//! service 托管（启动 / 端口 / 代理 / 健康检查 / 运行时提供）见 spec §5，后续步骤实现。
//!
//! 目录约定：`app_data_dir()/extensions/<extId>/manifest.json`。
//! 每个子目录是一个已安装扩展；manifest 损坏或缺失的目录会被标记为 invalid 返回，
//! 让扩展中心能展示「此扩展不可用」而非静默消失。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tauri::Manager;

/// 扩展运行时类型：web（纯前端）/ service（带后端服务）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExtensionRuntime {
    Web,
    Service,
}

impl Default for ExtensionRuntime {
    fn default() -> Self {
        ExtensionRuntime::Web
    }
}

/// service 扩展的后端声明（manifest.backend）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendSpec {
    /// 后端入口（相对扩展目录的路径）
    pub entry: String,
    /// 运行时引擎要求（宿主据此判断复用系统运行时或下载内置）
    pub engine: Option<EngineSpec>,
    /// 后端工作目录（相对扩展目录）
    pub cwd: Option<String>,
    /// 0 = 动态分配；固定端口需冲突检测
    pub port: Option<u16>,
    /// 健康检查路径（可选）
    pub health: Option<String>,
}

/// 后端运行时引擎要求（backend.engine）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineSpec {
    #[serde(rename = "type")]
    pub engine_type: String,
    #[serde(rename = "minVersion")]
    pub min_version: Option<String>,
}

/// window / drawer 建议尺寸（manifest.minSize）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinSize {
    pub w: f64,
    pub h: f64,
}

/// 扩展 manifest（manifest.json），对齐 spec §4。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionManifest {
    /// 唯一标识，反向域名
    pub id: String,
    pub name: String,
    pub version: String,
    /// 运行时：web（默认）| service
    #[serde(default)]
    pub runtime: ExtensionRuntime,
    /// 默认 / 主形态：module / view / window / drawer
    #[serde(default = "default_kind")]
    pub kind: String,
    /// 声明支持哪些形态
    #[serde(default)]
    pub surfaces: Vec<String>,
    /// app 形态下支持哪些打开方式（manifest 字段为 openIn）
    #[serde(default, rename = "openIn")]
    pub open_in: Vec<String>,
    /// 各形态入口：形态 → 相对扩展目录的路径
    #[serde(default)]
    pub entry: HashMap<String, String>,
    /// 能力申请，按需授权
    #[serde(default)]
    pub permissions: Vec<String>,
    /// 图标（相对扩展目录的路径）
    pub icon: Option<String>,
    /// window / drawer 建议尺寸（manifest 字段为 minSize）
    #[serde(default, rename = "minSize")]
    pub min_size: Option<MinSize>,
    /// service 专属：受托管的后端服务声明
    pub backend: Option<BackendSpec>,
    /// 一句话描述（列表展示用；spec §4 未列，作为可选补充字段）
    #[serde(default)]
    pub description: String,
}

fn default_kind() -> String {
    "view".to_string()
}

/// 已安装扩展的注册表项（返回给前端的展示结构）。
#[derive(Debug, Clone, Serialize)]
pub struct ExtensionEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    /// "web" | "service"
    pub runtime: String,
    /// 默认 / 主形态
    pub kind: String,
    pub surfaces: Vec<String>,
    pub open_in: Vec<String>,
    pub permissions: Vec<String>,
    pub description: String,
    /// 图标文件绝对路径（存在时才非空）
    pub icon: Option<String>,
    /// 扩展目录绝对路径
    pub dir: String,
    /// manifest 缺失 / 解析失败时为 true
    pub invalid: bool,
    /// invalid 时的原因（供扩展中心友好展示）
    pub error: Option<String>,
}

fn runtime_str(r: &ExtensionRuntime) -> &'static str {
    match r {
        ExtensionRuntime::Web => "web",
        ExtensionRuntime::Service => "service",
    }
}

/// 扩展根目录：`app_data_dir()/extensions/`
pub fn extensions_root(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("extensions"))
}

/// 扫描扩展根目录，解析每个子目录的 manifest.json。
/// 单个目录解析失败不影响整体扫描，只标记该目录为 invalid。
pub fn scan_extensions(app: &tauri::AppHandle) -> Result<Vec<ExtensionEntry>, String> {
    let root = extensions_root(app)?;
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    let dirs = std::fs::read_dir(&root).map_err(|e| e.to_string())?;
    for entry in dirs.flatten() {
        let path = entry.path();
        if path.is_dir() {
            entries.push(load_extension(&path));
        }
    }
    // 按名称排序（中文按 Unicode 码点），保证列表稳定
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

/// 读取扩展目录下的 manifest.json。
pub fn read_manifest(dir: &Path) -> Result<ExtensionManifest, String> {
    let content = std::fs::read_to_string(dir.join("manifest.json"))
        .map_err(|e| format!("读取 manifest.json 失败：{e}"))?;
    serde_json::from_str(&content).map_err(|e| format!("manifest 解析失败：{e}"))
}

/// 加载单个扩展目录为注册表项（永不 panic，损坏时返回 invalid 项）。
fn load_extension(dir: &Path) -> ExtensionEntry {
    let dir_str = dir.to_string_lossy().into_owned();
    let fallback_name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir_str.clone());

    let invalid = |error: String| ExtensionEntry {
        id: fallback_name.clone(),
        name: fallback_name,
        version: String::new(),
        runtime: "web".to_string(),
        kind: "view".to_string(),
        surfaces: Vec::new(),
        open_in: Vec::new(),
        permissions: Vec::new(),
        description: String::new(),
        icon: None,
        dir: dir_str.clone(),
        invalid: true,
        error: Some(error),
    };

    let manifest = match read_manifest(dir) {
        Ok(m) => m,
        Err(e) => return invalid(e),
    };

    let icon = manifest.icon.as_ref().and_then(|rel| {
        let p = dir.join(rel);
        p.is_file().then(|| p.to_string_lossy().into_owned())
    });

    ExtensionEntry {
        id: manifest.id,
        name: manifest.name,
        version: manifest.version,
        runtime: runtime_str(&manifest.runtime).to_string(),
        kind: manifest.kind,
        surfaces: manifest.surfaces,
        open_in: manifest.open_in,
        permissions: manifest.permissions,
        description: manifest.description,
        icon,
        dir: dir_str,
        invalid: false,
        error: None,
    }
}

/// 列出已安装扩展（Tauri 命令）。
#[tauri::command]
pub fn list_extensions(app: tauri::AppHandle) -> Result<Vec<ExtensionEntry>, String> {
    let entries = scan_extensions(&app)?;
    log::info!("扩展注册表扫描完成：{} 个扩展", entries.len());
    Ok(entries)
}

/// 注入到扩展入口 HTML 的桥脚本：在扩展 iframe 内挂载 `window.xhub`，
/// 所有方法经 `window.parent.postMessage` 发 RPC 请求给主窗口，主窗口再 invoke `xhub_call`。
/// 用普通 `<script>`（非 module）且在 `<head>` 靠前位置注入，保证早于扩展自身脚本执行。
const XHUB_BRIDGE_SCRIPT: &str = r#"
(function(){
  var pending={};var seq=0;
  function call(ns,method,args){
    return new Promise(function(resolve,reject){
      var id=++seq;pending[id]={resolve:resolve,reject:reject};
      window.parent.postMessage({__xhub:true,type:'call',id:id,namespace:ns,method:method,args:args},'*');
    });
  }
  window.addEventListener('message',function(e){
    var m=e.data;if(!m||m.__xhub!==true||m.type!=='result')return;
    var p=pending[m.id];if(!p)return;delete pending[m.id];
    if(m.ok){p.resolve(m.data);}
    else{var err=new Error(m.error&&m.error.message||'xhub error');err.code=m.error&&m.error.code;p.reject(err);}
  });
  window.xhub={
    runtime:{info:function(){return call('runtime','info',{});}},
    storage:{
      get:function(k){return call('storage','get',{key:k});},
      set:function(k,v){return call('storage','set',{key:k,value:v});},
      remove:function(k){return call('storage','remove',{key:k});},
      clear:function(){return call('storage','clear',{});}
    },
    data:{
      notes:{list:function(){return call('data','notes.list',{});},get:function(id){return call('data','notes.get',{id:id});}},
      todos:{list:function(){return call('data','todos.list',{});}},
      resources:{list:function(){return call('data','resources.list',{});}},
      usage:{summary:function(){return call('data','usage.summary',{});}}
    },
    service:{
      request:function(path,init){
        init=init||{};
        return call('service','request',{path:path,method:init.method,headers:init.headers,body:init.body})
          .then(function(res){
            return {
              status:res.status,
              headers:res.headers,
              text:function(){return Promise.resolve(res.body);},
              json:function(){return Promise.resolve(JSON.parse(res.body));}
            };
          });
      }
    }
  };
})();
"#;

/// 找到 `tag_open`（如 `<head`）首次出现后，其闭合 `>` 之后的字节位置。
fn find_tag_end(html: &str, tag_open: &str) -> Option<usize> {
    let start = html.find(tag_open)?;
    let rest = &html[start..];
    let gt = rest.find('>')?;
    Some(start + gt + 1)
}

/// 把桥脚本注入 HTML：优先插到 `<head...>` 开始标签之后（head 内第一个元素，
/// 早于扩展自身脚本执行）；无 head 则插到 `</head>` 前；再退到 body 前；都没有则插到最前。
fn inject_bridge(html: &str, bridge: &str) -> String {
    let script = format!("<script>{bridge}</script>");
    if let Some(pos) = find_tag_end(html, "<head") {
        return format!("{}{}{}", &html[..pos], script, &html[pos..]);
    }
    if let Some(pos) = html.find("</head>") {
        return format!("{}{}{}", &html[..pos], script, &html[pos..]);
    }
    if let Some(pos) = html.find("<body") {
        return format!("{}{}{}", &html[..pos], script, &html[pos..]);
    }
    format!("{script}{html}")
}

/// 读取某扩展某形态的入口 HTML，注入桥脚本后写到 `<扩展目录>/.xhub/<surface>.html`，
/// 返回该临时文件的绝对路径（前端 `convertFileSrc` 后作为 iframe src）。
///
/// 写临时文件到扩展目录内是为了让入口引用的相对资源（Vite 产物的 module script / css）
/// 与入口保持同 origin 加载，规避 srcdoc + asset protocol 的跨域 CORS 限制。
#[tauri::command]
pub fn read_extension_entry(
    app: tauri::AppHandle,
    id: String,
    surface: Option<String>,
) -> Result<String, String> {
    let dir = extensions_root(&app)?.join(&id);
    if !dir.is_dir() {
        return Err(format!("NOT_FOUND: 扩展 {id} 不存在"));
    }
    let manifest = read_manifest(&dir)?;

    // service 扩展：打开时懒启动后端（探活成功则后续 runtime.info 返回 serviceReady=true）
    if manifest.runtime == ExtensionRuntime::Service {
        if let Err(e) = crate::service::start_service(&app, &id) {
            // 不阻断前端加载：前端仍能打开，runtime.info 会返回 serviceReady=false
            log::warn!("service 扩展 {id} 后端启动失败: {e}");
        }
    }

    let surface = surface.unwrap_or_else(|| manifest.kind.clone());
    let rel = manifest
        .entry
        .get(&surface)
        .or_else(|| manifest.entry.get("view"))
        .ok_or_else(|| format!("NOT_FOUND: 扩展 {id} 没有 {surface} 入口"))?;
    let html_path = dir.join(rel);
    let html =
        std::fs::read_to_string(&html_path).map_err(|e| format!("IO_ERROR: 读取入口失败：{e}"))?;
    let injected = inject_bridge(&html, XHUB_BRIDGE_SCRIPT);

    let out_dir = dir.join(".xhub");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let out_path = out_dir.join(format!("{surface}.html"));
    std::fs::write(&out_path, injected).map_err(|e| e.to_string())?;
    log::info!("扩展入口就绪: {id} [{surface}] -> {}", out_path.display());
    Ok(out_path.to_string_lossy().into_owned())
}

/// 打开扩展的独立窗口（window 形态）。窗口 label 为 `ext-<扩展id>`，已存在则聚焦复用。
/// 窗口加载宿主 `index.html`（App.vue 按 `ext-` 前缀路由到 ExtensionWindow），
/// 内容仍走 iframe + 桥 API，与 view/module 同一条注入链路。
#[tauri::command]
pub fn open_extension_window(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let label = format!("ext-{id}");
    if let Some(win) = app.get_webview_window(&label) {
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }

    let dir = extensions_root(&app)?.join(&id);
    if !dir.is_dir() {
        return Err(format!("NOT_FOUND: 扩展 {id} 不存在"));
    }
    let manifest = read_manifest(&dir)?;
    let (w, h) = manifest
        .min_size
        .as_ref()
        .map(|m| (m.w.max(360.0), m.h.max(260.0)))
        .unwrap_or((800.0, 600.0));

    tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::App("index.html".into()))
        .title(manifest.name)
        .inner_size(w, h)
        .min_inner_size(360.0, 260.0)
        .additional_browser_args(crate::ADDITIONAL_BROWSER_ARGS)
        .build()
        .map_err(|e| e.to_string())?;

    log::info!("扩展窗口已打开: {id} [{label}] {w}x{h}");
    Ok(())
}

/// 卸载扩展：停止其 service 后端进程（如有）并删除扩展目录。
/// （卸载 UI 在 §12.7 扩展中心补全时接入，此处先落地服务端清理逻辑）
#[tauri::command]
pub fn uninstall_extension(app: tauri::AppHandle, id: String) -> Result<(), String> {
    crate::service::stop_service(&app, &id);
    let dir = extensions_root(&app)?.join(&id);
    if dir.is_dir() {
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    log::info!("扩展已卸载: {id}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_manifest(root: &Path, id: &str, manifest: serde_json::Value) {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn parses_web_manifest() {
        let dir = tempdir().unwrap();
        write_manifest(
            dir.path(),
            "com.x-hub.apidebug",
            serde_json::json!({
                "id": "com.x-hub.apidebug",
                "name": "API 调试助手",
                "version": "1.0.0",
                "runtime": "web",
                "kind": "view",
                "surfaces": ["module", "view"],
                "openIn": ["view", "window"],
                "entry": { "view": "./app/index.js" },
                "permissions": ["clipboard"],
                "icon": "./icon.svg"
            }),
        );

        let entry = load_extension(&dir.path().join("com.x-hub.apidebug"));
        assert!(!entry.invalid);
        assert_eq!(entry.id, "com.x-hub.apidebug");
        assert_eq!(entry.name, "API 调试助手");
        assert_eq!(entry.runtime, "web");
        assert_eq!(entry.kind, "view");
        assert_eq!(entry.surfaces, vec!["module", "view"]);
        assert_eq!(entry.open_in, vec!["view", "window"]);
        assert_eq!(entry.permissions, vec!["clipboard"]);
    }

    #[test]
    fn parses_service_manifest_with_backend() {
        let dir = tempdir().unwrap();
        write_manifest(
            dir.path(),
            "com.x-hub.dsh",
            serde_json::json!({
                "id": "com.x-hub.dsh",
                "name": "DeepSeek Harness",
                "version": "0.1.1",
                "runtime": "service",
                "kind": "view",
                "surfaces": ["view"],
                "openIn": ["view", "window"],
                "entry": { "view": "./entry/index.html" },
                "backend": {
                    "entry": "./service/index.js",
                    "engine": { "type": "node", "minVersion": "22" },
                    "cwd": "./service",
                    "port": 0,
                    "health": "/healthz"
                },
                "permissions": ["network", "fs", "process"]
            }),
        );

        let entry = load_extension(&dir.path().join("com.x-hub.dsh"));
        assert!(!entry.invalid);
        assert_eq!(entry.runtime, "service");

        // 直接解析 manifest，校验 backend 字段
        let raw = std::fs::read_to_string(dir.path().join("com.x-hub.dsh/manifest.json")).unwrap();
        let manifest: ExtensionManifest = serde_json::from_str(&raw).unwrap();
        let backend = manifest.backend.unwrap();
        assert_eq!(backend.entry, "./service/index.js");
        assert_eq!(backend.port, Some(0));
        assert_eq!(backend.health.as_deref(), Some("/healthz"));
        let engine = backend.engine.unwrap();
        assert_eq!(engine.engine_type, "node");
        assert_eq!(engine.min_version.as_deref(), Some("22"));
    }

    #[test]
    fn runtime_defaults_to_web_and_kind_to_view() {
        let dir = tempdir().unwrap();
        write_manifest(
            dir.path(),
            "com.x-hub.minimal",
            serde_json::json!({
                "id": "com.x-hub.minimal",
                "name": "最小扩展",
                "version": "0.0.1"
            }),
        );

        let raw = std::fs::read_to_string(dir.path().join("com.x-hub.minimal/manifest.json")).unwrap();
        let manifest: ExtensionManifest = serde_json::from_str(&raw).unwrap();
        assert_eq!(manifest.runtime, ExtensionRuntime::Web);
        assert_eq!(manifest.kind, "view");
    }

    #[test]
    fn broken_manifest_is_invalid_not_panic() {
        let dir = tempdir().unwrap();
        let ext = dir.path().join("com.x-hub.broken");
        std::fs::create_dir_all(&ext).unwrap();
        std::fs::write(ext.join("manifest.json"), "not valid json {{{").unwrap();

        let entry = load_extension(&ext);
        assert!(entry.invalid);
        assert!(entry.error.is_some());
        assert_eq!(entry.id, "com.x-hub.broken");
    }

    #[test]
    fn missing_manifest_is_invalid() {
        let dir = tempdir().unwrap();
        let ext = dir.path().join("com.x-hub.nomanifest");
        std::fs::create_dir_all(&ext).unwrap();

        let entry = load_extension(&ext);
        assert!(entry.invalid);
        assert!(entry.error.unwrap().contains("manifest.json"));
    }

    #[test]
    fn inject_bridge_inserts_before_head() {
        let html = "<html><head><title>t</title></head><body>x</body></html>";
        let out = inject_bridge(html, "BRIDGE");
        assert!(out.contains("<script>BRIDGE</script>"));
        assert!(out.find("<script>").unwrap() < out.find("<title>").unwrap());
        assert!(out.ends_with("</head><body>x</body></html>"));
    }

    #[test]
    fn inject_bridge_without_head_prepends() {
        let html = "<body>hi</body>";
        let out = inject_bridge(html, "BRIDGE");
        assert!(out.starts_with("<script>BRIDGE</script>"));
    }
}
