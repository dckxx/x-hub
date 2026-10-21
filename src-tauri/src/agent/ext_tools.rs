//! 扩展 AI 工具聚合与调用（manifest.agent.tools）。
//!
//! 「宿主不知道扩展里有什么数据」的解法是扩展**自描述**：manifest 声明
//! `agent.tools[]`（名称/描述/参数 JSON Schema/审批档位/返回值形状），本模块聚合
//! 已启用 service 扩展的声明，供 `extension_tool_list` / `extension_tool_call`
//! 两个内置 Agent 工具消费——对齐 MCP 的 tools/list + tools/call 模式。
//!
//! 一期仅 service 扩展可声明：后端由宿主托管常驻，AI 随时可调；web 扩展的
//! expose handler 活在扩展页面 iframe 里，页面没开就无人应答，不适合 AI 调用。
//!
//! 权限口径（计划 §5 铁律：AI 工具授权 ≠ 扩展 manifest 权限，两套体系）：
//! - 总开关 + 每扩展开关（`agent_ext_tools_enabled` / `agent_ext_tool_grants`）
//!   决定工具**进不进聚合清单**；
//! - 声明的 `level` 决定运行时**审批档**（只读档直跑 / ask 档弹卡 / 完全档直跑），
//!   由 turn 循环经 `AgentTool::approval_for` 的覆写逐调用取档。

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::AppHandle;

use super::approval::Level;

/// 宿主 AppHandle。`AgentTool::execute` 是同步签名（工具表全局单例），拿不到
/// 命令参数里的句柄；lib.rs setup 时注入一次。未注入（单测环境）时聚合为空、调用报错。
static HOST_APP: OnceLock<AppHandle> = OnceLock::new();

pub fn init(app: AppHandle) {
    let _ = HOST_APP.set(app);
}

fn app() -> Result<&'static AppHandle, String> {
    HOST_APP.get().ok_or_else(|| "INTERNAL: 宿主未就绪，无法访问扩展".to_string())
}

/// 一次可调用的扩展 AI 工具（聚合清单的条目）
#[derive(Debug, Clone)]
pub struct AvailableTool {
    pub ext_id: String,
    pub ext_name: String,
    pub tool: crate::extension::AgentToolDecl,
}

/// 每扩展授权：缺项 = 允许（扩展开声明 + 用户装了它即视为同意；运行时审批三档照常兜底）
fn ext_granted(cfg: &crate::config::AppConfig, ext_id: &str) -> bool {
    cfg.agent_ext_tool_grants.get(ext_id).copied().unwrap_or(true)
}

/// 聚合当前可被 AI 调用的扩展工具清单。
/// 过滤：宿主就绪 + 总开关开 + 扩展健康（未失效/未条件禁用/能力与依赖齐全）+
/// runtime=service + 每扩展开关未关。每次调用现扫目录（与扩展中心同一入口），
/// 扩展增删与 manifest 改动天然即时生效，无需另立缓存与失效机制。
pub fn list_available() -> Vec<AvailableTool> {
    let Some(app) = HOST_APP.get() else {
        return Vec::new();
    };
    let cfg = crate::config::load();
    if !cfg.agent_ext_tools_enabled {
        return Vec::new();
    }
    let Ok(entries) = crate::extension::scan_extensions(app) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in entries {
        if e.invalid || e.disabled || e.runtime != "service" {
            continue;
        }
        if !e.missing_capabilities.is_empty() || !e.missing_dependencies.is_empty() {
            continue;
        }
        if !ext_granted(&cfg, &e.id) {
            continue;
        }
        for tool in e.agent_tools {
            out.push(AvailableTool { ext_id: e.id.clone(), ext_name: e.name.clone(), tool });
        }
    }
    out
}

/// 查找一个可用工具（call 前校验「声明过且当前可调」的单一闸口）
pub fn find(ext_id: &str, tool: &str) -> Option<AvailableTool> {
    list_available()
        .into_iter()
        .find(|a| a.ext_id == ext_id && a.tool.name == tool)
}

/// 声明档位 → 审批档位（声明经 AgentToolDecl::validate 白名单校验，这里只兜底）
pub fn level_of(decl: &str) -> Level {
    match decl {
        "write" => Level::Write,
        "dangerous" => Level::Dangerous,
        _ => Level::Read,
    }
}

/// 会话级授权键：带「扩展/工具」，Ask 档「本会话内允许」按目标细粒度生效——
/// 批准了天气查询不代表批准同一扩展的删除操作
pub fn perm_key(ext_id: &str, tool: &str) -> String {
    format!("extension_tool_call:{ext_id}/{tool}")
}

/// 把参数 schema 的 properties 摘成一行人话（模型与 UI 都好读）：
/// `city(string, 必填) 城市名；days(integer) 预报天数`
fn describe_params(parameters: &Value) -> String {
    let Some(props) = parameters.get("properties").and_then(Value::as_object) else {
        return "无参数".to_string();
    };
    if props.is_empty() {
        return "无参数".to_string();
    }
    let required: std::collections::HashSet<&str> = parameters
        .get("required")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let parts: Vec<String> = props
        .iter()
        .map(|(name, schema)| {
            let ty = schema.get("type").and_then(Value::as_str).unwrap_or("any");
            let req = if required.contains(name.as_str()) { ", 必填" } else { "" };
            let desc = schema
                .get("description")
                .and_then(Value::as_str)
                .map(|d| format!(" {d}"))
                .unwrap_or_default();
            format!("{name}({ty}{req}){desc}")
        })
        .collect();
    parts.join("；")
}

/// 单工具的可读描述（extension_tool_list 的正文行）
pub fn describe_tool(a: &AvailableTool) -> String {
    let t = &a.tool;
    let mut line = format!(
        "- {}（{}）：{}",
        t.name,
        t.level,
        t.description.trim().trim_end_matches('。')
    );
    line.push_str(&format!("。参数：{}", describe_params(&t.parameters)));
    if !t.returns.trim().is_empty() {
        line.push_str(&format!("。返回：{}", t.returns.trim()));
    }
    line
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const CALL_TIMEOUT: Duration = Duration::from_secs(20);
/// 后端就绪等待（start_service 的探活在后台线程，返回 Ok ≠ 端口已监听）
const READY_TIMEOUT: Duration = Duration::from_secs(5);
/// 返回体硬上限：detail 会全文进 UI 与模型上下文，扩展后端是信任边界内的代码，
/// 但一个失控的「列出全部数据」足以把对话撑爆
const MAX_RESPONSE_BYTES: usize = 256 * 1024;

/// 扩展后端端点约定：POST /xhub/agent-tool，body `{"tool": "...", "args": {...}}`，
/// 2xx 响应体（任意 JSON）即工具结果；非 2xx 取 body 的 error/message 字段当报错。
/// 请求头带 X-XHub-Ext-Id 供后端日志溯源。
pub fn call(ext_id: &str, tool: &str, args: &Value) -> Result<Value, String> {
    let app = app()?;
    // 只放行「声明过且当前可调」的工具。服务端口本就是扩展自己的进程，不校验也通——
    // 这道闸防的是 manifest 与代码漂移（声明删了代码还在）和聚合侧状态（禁用/未授权）漏拦
    find(ext_id, tool)
        .ok_or_else(|| format!("NOT_FOUND: 扩展 {ext_id} 没有可用的 AI 工具 {tool}（未声明、已禁用或未授权 AI 调用）"))?;
    let port = ensure_service(app, ext_id)?;
    let url = format!("http://127.0.0.1:{port}/xhub/agent-tool");

    // turn 循环跑在 tokio worker 上，同步 execute 里不能 block_on（会 panic）：
    // 独立线程 + 自建 current_thread 运行时阻塞等待（与 get_weather 同款模式）
    let (tx, rx) = std::sync::mpsc::channel();
    let (ext, tool, args) = (ext_id.to_string(), tool.to_string(), args.clone());
    std::thread::spawn(move || {
        let res = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt.block_on(post_agent_tool(&url, &ext, &tool, &args)),
            Err(e) => Err(format!("INTERNAL: HTTP 运行时初始化失败: {e}")),
        };
        let _ = tx.send(res);
    });
    match rx.recv_timeout(CALL_TIMEOUT + CONNECT_TIMEOUT) {
        Ok(res) => res,
        Err(_) => Err(format!(
            "IO_ERROR: 扩展后端 {} 内没有响应（超过 {} 秒），可稍后重试",
            ext_id,
            (CALL_TIMEOUT + CONNECT_TIMEOUT).as_secs()
        )),
    }
}

async fn post_agent_tool(url: &str, ext_id: &str, tool: &str, args: &Value) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(CALL_TIMEOUT)
        .build()
        .map_err(|e| format!("INTERNAL: HTTP 客户端构建失败: {e}"))?;
    let resp = client
        .post(url)
        .header("X-XHub-Ext-Id", ext_id)
        .json(&json!({ "tool": tool, "args": args }))
        .send()
        .await
        .map_err(|e| format!("IO_ERROR: 扩展后端调用失败: {e}"))?;
    let status = resp.status();
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("IO_ERROR: 读取扩展后端响应失败: {e}"))?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(format!(
            "IO_ERROR: 扩展后端返回 {} KB，超过 {} KB 上限——请在扩展侧做分页/截断",
            bytes.len() / 1024,
            MAX_RESPONSE_BYTES / 1024
        ));
    }
    let v: Value = serde_json::from_slice(&bytes)
        .map_err(|_| "IO_ERROR: 扩展后端返回的不是合法 JSON".to_string())?;
    if !status.is_success() {
        let msg = v
            .get("error")
            .or_else(|| v.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("未知错误");
        return Err(format!("扩展后端返回 {status}: {msg}"));
    }
    Ok(v)
}

/// 拿到扩展后端端口：幂等复用已有进程；没在跑则现场拉起（start_service 自带
/// service:execute / network 权限闸，被用户关掉后端时这里直接报权限错）。
/// 返回前等端口真正可连——探活是后台线程，start_service 返回 Ok 不代表已监听。
fn ensure_service(app: &AppHandle, ext_id: &str) -> Result<u16, String> {
    let port = crate::service::start_service(app, ext_id)?;
    let deadline = Instant::now() + READY_TIMEOUT;
    while Instant::now() < deadline {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Ok(port);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(format!(
        "IO_ERROR: 扩展 {ext_id} 的本地后端未就绪（{READY_TIMEOUT:?}内端口 {port} 未监听），请稍后重试或到扩展中心查看后端日志"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn decl(name: &str, level: &str, params: Value) -> crate::extension::AgentToolDecl {
        crate::extension::AgentToolDecl {
            name: name.into(),
            description: "测试工具".into(),
            level: level.into(),
            parameters: params,
            returns: String::new(),
        }
    }

    #[test]
    fn level_of_maps_three_tiers() {
        assert_eq!(level_of("read"), Level::Read);
        assert_eq!(level_of("write"), Level::Write);
        assert_eq!(level_of("dangerous"), Level::Dangerous);
        // 声明经 validate 白名单挡过，这里兜底不 panic 即可
        assert_eq!(level_of("nonsense"), Level::Read);
    }

    #[test]
    fn perm_key_isolates_per_ext_and_tool() {
        assert_eq!(perm_key("com.a", "query"), "extension_tool_call:com.a/query");
        assert_ne!(perm_key("com.a", "query"), perm_key("com.a", "delete"));
        assert_ne!(perm_key("com.a", "query"), perm_key("com.b", "query"));
    }

    #[test]
    fn describe_tool_includes_params_and_returns() {
        let a = AvailableTool {
            ext_id: "com.x".into(),
            ext_name: "天气".into(),
            tool: crate::extension::AgentToolDecl {
                name: "query".into(),
                description: "查询城市当前天气。".into(),
                level: "read".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "city": {"type": "string", "description": "城市名"},
                        "days": {"type": "integer"}
                    },
                    "required": ["city"]
                }),
                returns: "{ temp, humidity }".into(),
            },
        };
        let s = describe_tool(&a);
        assert!(s.contains("- query（read）：查询城市当前天气"));
        assert!(s.contains("city(string, 必填) 城市名"));
        assert!(s.contains("days(integer)"));
        assert!(s.contains("返回：{ temp, humidity }"));
    }

    #[test]
    fn describe_params_handles_empty_and_missing() {
        assert_eq!(describe_params(&Value::Null), "无参数");
        assert_eq!(describe_params(&json!({"type":"object","properties":{}})), "无参数");
    }

    #[test]
    fn ext_granted_defaults_to_allow() {
        let cfg = crate::config::AppConfig::default();
        assert!(ext_granted(&cfg, "com.any"));
    }

    #[test]
    fn list_available_is_empty_without_host() {
        // 单测环境没注入 HOST_APP：聚合必须为空而不是 panic
        assert!(list_available().is_empty());
    }
}
