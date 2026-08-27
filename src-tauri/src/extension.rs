//! 扩展系统：manifest 解析 + 本地目录扫描注册表（spec §12 第 1 步）。
//!
//! 第 1 步只做「扫描 + 解析 + 列出」，不涉及安装 / 卸载 / 打开 / service 进程托管。
//! service 托管（启动 / 端口 / 代理 / 健康检查 / 运行时提供）见 spec §5，后续步骤实现。
//!
//! 目录约定：`data_root()/extensions/<extId>/manifest.json`。
//! 每个子目录是一个已安装扩展；manifest 损坏或缺失的目录会被标记为 invalid 返回，
//! 让扩展中心能展示「此扩展不可用」而非静默消失。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
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

/// 条件禁用（manifest.disabled）：满足任一条件则扩展被禁用（扫描时求值）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisableCondition {
    /// 当前平台匹配则禁用："windows" | "macos" | "linux"
    #[serde(default)]
    pub platform: Option<String>,
}

/// 扩展动作（manifest.actions）：预定义的快捷动作，点击打开对应形态（能力注入）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionAction {
    pub id: String,
    pub title: String,
    /// 点击打开哪个形态：module / view / window / drawer
    pub surface: String,
}

/// 宿主当前已实现的能力全集（`namespace.method`），供 manifest.requires 校验。
/// 来源 = 桥 API 能力表（xhub_api::capabilities）。
pub fn host_capabilities() -> std::collections::HashSet<String> {
    crate::xhub_api::capabilities()
        .iter()
        .map(|c| format!("{}.{}", c.namespace, c.method))
        .collect()
}

/// 求值条件禁用：任一条件命中返回 true。
fn condition_matches(cond: &DisableCondition) -> bool {
    if let Some(platform) = cond.platform.as_deref() {
        if platform.eq_ignore_ascii_case(std::env::consts::OS) {
            return true;
        }
    }
    false
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
    /// 依赖的宿主能力（`namespace.method`，如 "data.notes.list"）；宿主缺失则标 missing_capabilities
    #[serde(default)]
    pub requires: Vec<String>,
    /// 依赖的其它扩展 id（manifest 字段为 dependsOn）；未安装则标 missing_dependencies
    #[serde(default, rename = "dependsOn")]
    pub depends_on: Vec<String>,
    /// 条件禁用（manifest 字段为 disabled）
    #[serde(default)]
    pub disabled: Option<DisableCondition>,
    /// 暴露给其它扩展调用的方法名（跨扩展调用白名单，manifest 字段为 expose）
    #[serde(default)]
    pub expose: Vec<String>,
    /// 快捷动作（manifest 字段为 actions，能力注入）
    #[serde(default)]
    pub actions: Vec<ExtensionAction>,
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
    /// 扩展默认配置（对象）；用户覆盖存 `.config.json`，读取时用户覆盖优先（配置分层）
    #[serde(default)]
    pub config: Map<String, Value>,
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
    /// 条件禁用求值结果：true = 被禁用（前端灰显 / 不展示入口）
    pub disabled: bool,
    /// 缺失的宿主能力（manifest.requires 中宿主未实现的）
    pub missing_capabilities: Vec<String>,
    /// 缺失的依赖扩展 id（manifest.dependsOn 中未安装的）
    pub missing_dependencies: Vec<String>,
    /// 扩展声明的依赖扩展 id（manifest.dependsOn 原样，供前端展示依赖关系）
    pub depends_on: Vec<String>,
    /// 暴露给其它扩展调用的方法名（manifest.expose）
    pub expose: Vec<String>,
    /// 快捷动作（manifest.actions）
    pub actions: Vec<ExtensionAction>,
}

fn runtime_str(r: &ExtensionRuntime) -> &'static str {
    match r {
        ExtensionRuntime::Web => "web",
        ExtensionRuntime::Service => "service",
    }
}

/// 扩展根目录：`data_root()/extensions/`
/// 注意必须用 `paths::data_root()`（便携版跟随 exe 目录\data），
/// 不能用 `app_data_dir()`——后者永远返回 %APPDATA% 下的目录，便携版会装到用户目录。
pub fn extensions_root(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let _ = app;
    Ok(crate::paths::data_root().join("extensions"))
}

/// 是否为隐藏目录（`.` 开头）。扩展 id 不以 `.` 开头；`.backup/`、`.tmp-update/`
/// 等内部暂存目录在扫描 / stamp 时一律跳过，避免被当成扩展或干扰热更新戳。
fn is_hidden_dir(p: &std::path::Path) -> bool {
    p.file_name()
        .map(|n| n.to_string_lossy().starts_with('.'))
        .unwrap_or(false)
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
        if path.is_dir() && !is_hidden_dir(&path) {
            entries.push(load_extension(&path));
        }
    }

    // dependsOn 后处理：收集已安装的 valid 扩展 id，回填 missing_dependencies
    let installed_ids: std::collections::HashSet<String> = entries
        .iter()
        .filter(|e| !e.invalid)
        .map(|e| e.id.clone())
        .collect();
    for entry in &mut entries {
        if entry.invalid {
            continue;
        }
        entry.missing_dependencies = entry
            .depends_on
            .iter()
            .filter(|d| !installed_ids.contains(*d))
            .cloned()
            .collect();
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
        disabled: false,
        missing_capabilities: Vec::new(),
        missing_dependencies: Vec::new(),
        depends_on: Vec::new(),
        expose: Vec::new(),
        actions: Vec::new(),
    };

    let manifest = match read_manifest(dir) {
        Ok(m) => m,
        Err(e) => return invalid(e),
    };

    let icon = manifest.icon.as_ref().and_then(|rel| {
        let p = dir.join(rel);
        p.is_file().then(|| p.to_string_lossy().into_owned())
    });

    // 条件禁用求值（manifest.disabled）
    let disabled = manifest
        .disabled
        .as_ref()
        .map(condition_matches)
        .unwrap_or(false);

    // requires 能力校验：宿主未实现的能力进 missing_capabilities
    let host_caps = host_capabilities();
    let missing_capabilities: Vec<String> = manifest
        .requires
        .iter()
        .filter(|r| !host_caps.contains(*r))
        .cloned()
        .collect();

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
        disabled,
        missing_capabilities,
        missing_dependencies: Vec::new(), // 后处理填（见 scan_extensions）
        depends_on: manifest.depends_on,
        expose: manifest.expose,
        actions: manifest.actions,
    }
}

/// 列出已安装扩展（Tauri 命令）。
#[tauri::command]
pub fn list_extensions(app: tauri::AppHandle) -> Result<Vec<ExtensionEntry>, String> {
    let entries = scan_extensions(&app)?;
    log::info!("扩展注册表扫描完成：{} 个扩展", entries.len());
    Ok(entries)
}

/// 扩展目录内容戳：对所有 manifest.json 的「路径 + 修改时间」做 FNV-1a 哈希。
/// 前端扩展中心轮询此戳，变化即刷新列表（运行时热更新：新装/卸载/改 manifest 无需重启）。
#[tauri::command]
pub fn extensions_stamp(app: tauri::AppHandle) -> Result<u64, String> {
    let root = extensions_root(&app)?;
    if !root.exists() {
        return Ok(0);
    }
    let mut hash: u64 = 1469598103934665603; // FNV-1a offset basis
    let mut dirs: Vec<_> = std::fs::read_dir(&root)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter(|e| e.path().is_dir() && !is_hidden_dir(&e.path()))
        .collect();
    dirs.sort_by_key(|e| e.path());
    for entry in dirs {
        let manifest = entry.path().join("manifest.json");
        if let Ok(meta) = std::fs::metadata(&manifest) {
            if let Ok(mtime) = meta.modified() {
                let secs = mtime
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                for b in manifest.to_string_lossy().bytes().chain(secs.to_le_bytes()) {
                    hash ^= b as u64;
                    hash = hash.wrapping_mul(1099511628211);
                }
            }
        }
    }
    Ok(hash)
}

/// 注入到扩展入口 HTML 的桥脚本：在扩展 iframe 内挂载 `window.xhub`，
/// 所有方法经 `window.parent.postMessage` 发 RPC 请求给主窗口，主窗口再 invoke `xhub_call`。
/// 用普通 `<script>`（非 module）且在 `<head>` 靠前位置注入，保证早于扩展自身脚本执行。
const XHUB_BRIDGE_SCRIPT: &str = r#"
(function(){
  var pending={};var seq=0;
  var listeners={};
  function call(ns,method,args){
    return new Promise(function(resolve,reject){
      var id=++seq;pending[id]={resolve:resolve,reject:reject};
      window.parent.postMessage({__xhub:true,type:'call',id:id,namespace:ns,method:method,args:args},'*');
    });
  }
  function emit(event,payload){
    var arr=listeners[event];if(!arr)return;
    for(var i=0;i<arr.length;i++){try{arr[i](payload);}catch(e){}}
  }
  // 把宿主主题令牌写到 documentElement 的 --xhub-* CSS 变量，扩展 CSS 直接引用即可跟随主题
  function applyTheme(theme){
    var root=document.documentElement;if(!root)return;
    var dark=!!(theme&&theme.mode==='dark');
    root.setAttribute('data-xhub-theme',dark?'dark':'light');
    if(theme&&theme.preset){root.setAttribute('data-xhub-preset',theme.preset);}
    var t=(theme&&theme.tokens)||{};
    var map={
      '--xhub-accent':t.accent,
      '--xhub-brand':t.brand,
      '--xhub-brand-soft':t.brandSoft,
      '--xhub-bg-page':t.bgPage,
      '--xhub-bg-card':t.bgCard,
      '--xhub-surface':t.surface,
      '--xhub-text-1':t.text1,
      '--xhub-text-2':t.text2,
      '--xhub-text-3':t.text3,
      '--xhub-border':t.border,
      '--xhub-red':t.red,
      '--xhub-green':t.green,
      '--xhub-yellow':t.yellow,
      '--xhub-blue':t.blue,
      '--xhub-orange':t.orange,
      '--xhub-radius-lg':t.radiusLg
    };
    for(var k in map){if(map[k]!=null&&map[k]!==''){root.style.setProperty(k,map[k]);}}
  }
  window.addEventListener('message',function(e){
    var m=e.data;if(!m||m.__xhub!==true)return;
    if(m.type==='result'){
      var p=pending[m.id];if(!p)return;delete pending[m.id];
      if(m.ok){p.resolve(m.data);}
      else{var err=new Error(m.error&&m.error.message||'xhub error');err.code=m.error&&m.error.code;p.reject(err);}
    }else if(m.type==='theme'){
      applyTheme(m.theme);emit('theme-changed',m.theme);
    }else if(m.type==='event'){
      emit(m.event,m.payload);
    }else if(m.type==='xhub-call-result'){
      var p=pending[m.id];if(!p)return;delete pending[m.id];
      if(m.ok){p.resolve(m.data);}
      else{var err=new Error(m.error&&m.error.message||'call error');err.code=m.error&&m.error.code;p.reject(err);}
    }else if(m.type==='xhub-call-req'){
      var h=exposed[m.method];
      if(!h){window.parent.postMessage({__xhub:true,type:'xhub-call-result',id:m.id,ok:false,error:{message:'method not exposed: '+m.method}},'*');return;}
      var done=function(data){window.parent.postMessage({__xhub:true,type:'xhub-call-result',id:m.id,ok:true,data:data},'*');};
      var fail=function(err){window.parent.postMessage({__xhub:true,type:'xhub-call-result',id:m.id,ok:false,error:{message:String(err&&err.message||err)}},'*');};
      try{Promise.resolve(h(m.payload)).then(done).catch(fail);}catch(err){fail(err);}
    }
  });
  // 本扩展暴露给其它扩展调用的方法（跨扩展调用）
  var exposed={};
  window.xhub={
    runtime:{
      info:function(){return call('runtime','info',{});},
      open:function(surface){window.parent.postMessage({__xhub:true,type:'open',surface:surface||'view'},'*');return Promise.resolve();},
      callExtension:function(targetId,method,payload){
        return new Promise(function(resolve,reject){
          var id=++seq;pending[id]={resolve:resolve,reject:reject};
          window.parent.postMessage({__xhub:true,type:'xhub-call',id:id,targetId:targetId,method:method,payload:payload},'*');
        });
      }
    },
    storage:{
      get:function(k){return call('storage','get',{key:k});},
      set:function(k,v){return call('storage','set',{key:k,value:v});},
      remove:function(k){return call('storage','remove',{key:k});},
      clear:function(){return call('storage','clear',{});}
    },
    config:{
      get:function(k){return call('config','get',{key:k});},
      set:function(k,v){return call('config','set',{key:k,value:v});},
      remove:function(k){return call('config','remove',{key:k});},
      all:function(){return call('config','all',{});}
    },
    sharedStorage:{
      get:function(k){return call('sharedStorage','get',{key:k});},
      set:function(k,v){return call('sharedStorage','set',{key:k,value:v});},
      remove:function(k){return call('sharedStorage','remove',{key:k});}
    },
    data:{
      notes:{list:function(){return call('data','notes.list',{});},get:function(id){return call('data','notes.get',{id:id});}},
      todos:{list:function(){return call('data','todos.list',{});}},
      resources:{list:function(){return call('data','resources.list',{});}}
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
    },
    theme:{
      get:function(){return call('theme','get',{});}
    },
    events:{
      on:function(event,handler){
        (listeners[event]=listeners[event]||[]).push(handler);
        return function(){window.xhub.events.off(event,handler);};
      },
      off:function(event,handler){
        var arr=listeners[event];if(!arr)return;
        var i=arr.indexOf(handler);if(i>=0){arr.splice(i,1);}
      },
      emit:function(event,payload){
        window.parent.postMessage({__xhub:true,type:'xhub-emit',event:event,payload:payload},'*');
        return Promise.resolve();
      }
    },
    expose:function(method,handler){
      exposed[method]=handler;
    }
  };
  // 加载即拉取一次主题，确保首帧就与宿主一致
  call('theme','get',{}).then(applyTheme).catch(function(){});
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

/// 读取某扩展某形态的入口 HTML，注入桥脚本后写到 `<扩展目录>/.xhpack/<surface>.html`，
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
    log::info!("扩展入口读取: {id} [{surface}] dir={} html={}", dir.display(), html_path.display());
    let html = std::fs::read_to_string(&html_path).map_err(|e| {
        log::error!("扩展入口读取失败: {id} [{surface}] {} -> {e}", html_path.display());
        format!("IO_ERROR: 读取入口失败：{e}")
    })?;
    let injected = inject_bridge(&html, XHUB_BRIDGE_SCRIPT);

    let out_dir = dir.join(".xhpack");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let out_path = out_dir.join(format!("{surface}.html"));
    std::fs::write(&out_path, injected).map_err(|e| e.to_string())?;
    // 兜底日志：记录最终交给前端 convertFileSrc 的绝对路径，白屏排查时据此核对
    // asset 协议作用域（$APPDATA/** + 启动时 allow_directory(data_root)）是否覆盖该路径
    log::info!("扩展入口就绪: {id} [{surface}] -> {}", out_path.display());
    Ok(out_path.to_string_lossy().into_owned())
}

/// Tauri 窗口 label 只允许字母数字与 `-`/`/`/`:`/`_`；扩展 id 形如反向域名
/// `com.x-hub.ctool`（含点号），直接作为 label 会报
/// "Window labels must only include alphanumeric characters"。
/// 这里用 base64url（字符集恰好∈合法集合，且无填充）编码后拼 `ext-` 前缀，
/// 前端 `ExtensionWindow.vue` 再解码回真实 id。
const B64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn ext_window_label(id: &str) -> String {
    let bytes = id.as_bytes();
    let mut out = String::from("ext-");
    let mut i = 0usize;
    while i < bytes.len() {
        let b0 = bytes[i] as u32;
        let b1 = if i + 1 < bytes.len() { bytes[i + 1] as u32 } else { 0 };
        let b2 = if i + 2 < bytes.len() { bytes[i + 2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64URL[((n >> 18) & 63) as usize] as char);
        out.push(B64URL[((n >> 12) & 63) as usize] as char);
        if i + 1 < bytes.len() {
            out.push(B64URL[((n >> 6) & 63) as usize] as char);
        }
        if i + 2 < bytes.len() {
            out.push(B64URL[(n & 63) as usize] as char);
        }
        i += 3;
    }
    out
}

/// 打开扩展的独立窗口（window 形态）。窗口 label 为 `ext-<id 的 base64url>`，已存在则聚焦复用。
/// 窗口加载宿主 `index.html`（App.vue 按 `ext-` 前缀路由到 ExtensionWindow），
/// 内容仍走 iframe + 桥 API，与 view/module 同一条注入链路。
/// 必须 async：同步命令运行在主线程，会与 WebviewWindow 创建互相阻塞（死锁），
/// 与 detach_sticky 等建窗命令同一约束。
#[tauri::command]
pub async fn open_extension_window(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let label = ext_window_label(&id);
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

/// 递归复制目录
pub fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let s = entry.path();
        let d = dst.join(entry.file_name());
        if s.is_dir() {
            copy_dir_recursive(&s, &d)?;
        } else {
            std::fs::copy(&s, &d)?;
        }
    }
    Ok(())
}

// ---------- 权限授权（运行时逐项开关） ----------

/// 权限覆盖文件路径：`<扩展目录>/.permissions.json`（只存用户显式关闭的权限，默认授权）
fn permissions_path(app: &tauri::AppHandle, ext_id: &str) -> Result<std::path::PathBuf, String> {
    Ok(extensions_root(app)?.join(ext_id).join(".permissions.json"))
}

fn read_permission_overrides(app: &tauri::AppHandle, ext_id: &str) -> Map<String, Value> {
    let path = match permissions_path(app, ext_id) {
        Ok(p) => p,
        Err(_) => return Map::new(),
    };
    if !path.exists() {
        return Map::new();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|c| serde_json::from_str::<Value>(&c).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

fn write_permission_overrides(
    app: &tauri::AppHandle,
    ext_id: &str,
    map: &Map<String, Value>,
) -> Result<(), String> {
    let path = permissions_path(app, ext_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content =
        serde_json::to_string_pretty(&Value::Object(map.clone())).map_err(|e| e.to_string())?;
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

/// 某权限是否被授予：manifest 声明后默认授权，除非用户显式关闭。
pub fn permission_granted(app: &tauri::AppHandle, ext_id: &str, perm: &str) -> bool {
    let overrides = read_permission_overrides(app, ext_id);
    overrides
        .get(perm)
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

/// 查询扩展权限状态：manifest 声明的权限 → 是否授予。
#[tauri::command]
pub fn get_extension_permissions(
    app: tauri::AppHandle,
    id: String,
) -> Result<HashMap<String, bool>, String> {
    let dir = extensions_root(&app)?.join(&id);
    let manifest = read_manifest(&dir)?;
    let overrides = read_permission_overrides(&app, &id);
    let mut result = HashMap::new();
    for p in manifest.permissions {
        let granted = overrides.get(&p).and_then(|v| v.as_bool()).unwrap_or(true);
        result.insert(p, granted);
    }
    Ok(result)
}

/// 设置扩展某权限开关（仅记录「关闭」项；开启则删除覆盖）。
#[tauri::command]
pub fn set_extension_permission(
    app: tauri::AppHandle,
    id: String,
    permission: String,
    granted: bool,
) -> Result<(), String> {
    let mut overrides = read_permission_overrides(&app, &id);
    if granted {
        overrides.remove(&permission);
    } else {
        overrides.insert(permission.clone(), Value::Bool(false));
    }
    write_permission_overrides(&app, &id, &overrides)?;
    log::info!("扩展权限更新: {id} {permission} granted={granted}");
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

    #[test]
    fn condition_matches_platform() {
        let cond = DisableCondition {
            platform: Some("windows".to_string()),
        };
        assert_eq!(condition_matches(&cond), cfg!(target_os = "windows"));
        let empty = DisableCondition { platform: None };
        assert!(!condition_matches(&empty));
    }

    #[test]
    fn load_extension_computes_requires_and_disabled() {
        let dir = tempdir().unwrap();
        write_manifest(
            dir.path(),
            "com.x-hub.caps",
            serde_json::json!({
                "id": "com.x-hub.caps",
                "name": "能力探测",
                "version": "1.0.0",
                "requires": ["data.notes.list", "data.notes.create"],
                "dependsOn": ["com.x-hub.other"],
                "disabled": { "platform": "not-a-real-os" }
            }),
        );

        let entry = load_extension(&dir.path().join("com.x-hub.caps"));
        assert!(!entry.invalid);
        // data.notes.list 已实现 → 不缺失
        assert!(!entry.missing_capabilities.contains(&"data.notes.list".to_string()));
        // data.notes.create 未实现 → 缺失
        assert!(entry.missing_capabilities.contains(&"data.notes.create".to_string()));
        // platform 不匹配 → 不禁用
        assert!(!entry.disabled);
        // dependsOn 原样记录（缺失依赖在 scan_extensions 后处理）
        assert_eq!(entry.depends_on, vec!["com.x-hub.other".to_string()]);
    }

    #[test]
    fn ext_window_label_is_valid_and_roundtrips() {
        // 窗口 label 只允许字母数字与 -/_ 等字符；扩展 id 含点号，必须可逆编码
        let cases = ["com.x-hub.ctool", "com.x-hub.hello-service", "abc", "windows"];
        for id in cases {
            let label = ext_window_label(id);
            assert!(label.starts_with("ext-"), "label={label} id={id}");
            let stripped = &label["ext-".len()..];
            assert!(
                !stripped.is_empty()
                    && stripped
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '/' || c == ':'),
                "invalid chars in label={label} id={id}"
            );
        }
        assert_eq!(ext_window_label("com.x-hub.ctool"), "ext-Y29tLngtaHViLmN0b29s");
        // 同一 id 编码稳定
        assert_eq!(ext_window_label("com.x-hub.ctool"), ext_window_label("com.x-hub.ctool"));
    }
}
