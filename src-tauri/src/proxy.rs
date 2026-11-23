//! `/svc/<extId>/*` 反向代理（spec §5 代理转发）。
//!
//! 宿主启动一个监听 `127.0.0.1` 动态端口的 HTTP 服务器，把 `/svc/<extId>/<rest>` 转发到
//! 对应 service 扩展后端 `127.0.0.1:<port>/<rest>`。请求必须带路径中的随机令牌，
//! Origin 必须与该扩展独立来源一致；CORS 只放行这一来源。
//!
//! 支持非流式 HTTP 转发与 WebSocket 升级。停止服务时撤销代理令牌。

use crate::service::service_port;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::header::{CONNECTION, UPGRADE};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::convert::Infallible;
use std::collections::HashMap;
use std::sync::Mutex;

/// 代理服务器端口（0 = 未启动）
pub struct ProxyState {
    pub port: u16,
    tokens: Mutex<HashMap<String, String>>,
}

impl ProxyState {
    pub fn new(port: u16) -> Self { Self { port, tokens: Mutex::new(HashMap::new()) } }

    pub fn revoke(&self, id: &str) {
        if let Ok(mut tokens) = self.tokens.lock() { tokens.remove(id); }
    }

    pub fn prefix(&self, id: &str) -> Result<String, String> {
        let mut tokens = self.tokens.lock().map_err(|e| e.to_string())?;
        if !tokens.contains_key(id) {
            let mut bytes = [0u8; 32];
            getrandom::fill(&mut bytes).map_err(|e| format!("代理凭据生成失败: {e}"))?;
            tokens.insert(id.to_string(), bytes.iter().map(|b| format!("{b:02x}")).collect());
        }
        Ok(format!("http://127.0.0.1:{}/svc/{id}/{}", self.port, tokens[id]))
    }

    fn authorize<B>(&self, req: &Request<B>) -> Option<(String, String)> {
        let (id, rest) = parse_proxy_path(req.uri().path())?;
        let (token, suffix) = rest.trim_start_matches('/').split_once('/').unwrap_or((rest.trim_start_matches('/'), ""));
        let tokens = self.tokens.lock().ok()?;
        if tokens.get(&id).map(String::as_str) != Some(token) { return None; }
        let origin = req.headers().get("origin")?.to_str().ok()?;
        if origin != crate::ext_protocol::origin(&id) { return None; }
        Some((id, format!("/{suffix}")))
    }
}

/// 启动反向代理服务器，返回监听端口。内部 spawn accept 循环。
pub async fn start(app: tauri::AppHandle) -> Result<u16, String> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    tokio::spawn(accept_loop(listener, app));
    log::info!("扩展反向代理已启动: 127.0.0.1:{port}");
    Ok(port)
}

async fn accept_loop(listener: tokio::net::TcpListener, app: tauri::AppHandle) {
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(x) => x,
            Err(_) => continue,
        };
        let app2 = app.clone();
        tokio::spawn(async move {
            let io = TokioIo::new(stream);
            if let Err(e) = http1::Builder::new()
                .serve_connection(io, service_fn(move |req| {
                    let app3 = app2.clone();
                    async move { handle(&app3, req).await }
                }))
                .with_upgrades().await
            {
                // 连接结束/客户端断开属正常，debug 记录避免噪音
                log::debug!("代理连接结束: {e}");
            }
        });
    }
}

fn add_cors(resp: &mut Response<BoxBody<Bytes, Infallible>>, origin: &str) {
    let h = resp.headers_mut();
    if let Ok(origin) = origin.parse() { h.insert("Access-Control-Allow-Origin", origin); }
    let _ = h.insert("Vary", "Origin".parse().unwrap());
    let _ = h.insert("Access-Control-Allow-Methods", "GET, POST, PUT, PATCH, DELETE, OPTIONS".parse().unwrap());
    let _ = h.insert("Access-Control-Allow-Headers", "*".parse().unwrap());
}

fn error_response(status: StatusCode, msg: &str) -> Response<BoxBody<Bytes, Infallible>> {
    let mut resp = Response::new(BoxBody::new(Full::new(Bytes::from(msg.to_string()))));
    *resp.status_mut() = status;
    resp
}

/// 解析 `/svc/<extId>/<rest>` → `(extId, rest)`；rest 以 `/` 开头（无 rest 时为 `/`）。
fn parse_proxy_path(path: &str) -> Option<(String, String)> {
    let rest = path.strip_prefix("/svc/")?;
    if rest.is_empty() {
        return None;
    }
    match rest.find('/') {
        Some(idx) => {
            let ext = &rest[..idx];
            if ext.is_empty() {
                return None;
            }
            Some((ext.to_string(), rest[idx..].to_string()))
        }
        None => Some((rest.to_string(), "/".to_string())),
    }
}

/// hop-by-hop 头（转发时剔除；Host 在请求构造时按可信后端地址重建）
fn is_hop_by_hop(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "transfer-encoding"
            | "upgrade"
            | "proxy-connection"
            | "host"
    )
}

fn backend_request_builder(method: Method, uri: &hyper::Uri) -> hyper::http::request::Builder {
    // hyper 的底层 conn API 不会补 Host；缺失时 Node 的 HTTP/1.1 服务返回 400。
    Request::builder().method(method)
        .uri(uri.path_and_query().map(|p| p.as_str()).unwrap_or("/"))
        .header("Host", uri.authority().map(|a| a.as_str()).unwrap_or("127.0.0.1"))
}

async fn handle(
    app: &tauri::AppHandle,
    req: Request<Incoming>,
) -> Result<Response<BoxBody<Bytes, Infallible>>, Infallible> {
    use tauri::Manager;
    // 命名空间：`/svc/…` = service 后端反向代理；其余 `/<extId>/<rel>` = 扩展内容。
    // 扩展内容仅 macOS 实际走这条通道——WKWebView 不派发「自定义协议主窗里另一个
    // 自定义协议 iframe」的子帧请求（见 ext_protocol::origin 注释），Windows 继续用
    // xhub-ext: 协议。无需代理令牌：内容本身是公开 Web 资产，安全靠 serve 侧校验
    // （Host 必须匹配扩展身份 + canonicalize 防越界 + 敏感文件拦截，顺带挡住 DNS
    // 重绑定——外部域名的 Host 过不了身份比对）。
    if !req.uri().path().starts_with("/svc/") {
        let host = req
            .headers()
            .get("host")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let referer = req
            .headers()
            .get("referer")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let raw_path = req.uri().path().trim_start_matches('/');
        return Ok(ext_content_response(app, host, raw_path, referer.as_deref()));
    }
    let Some((ext_id, rest)) = app.state::<ProxyState>().authorize(&req) else {
        return Ok(error_response(StatusCode::FORBIDDEN, "访问扩展服务需要有效凭据与匹配的来源"));
    };
    let origin = crate::ext_protocol::origin(&ext_id);
    let mut response = forward(app, req, ext_id, rest).await?;
    add_cors(&mut response, &origin);
    Ok(response)
}

/// 扩展内容响应：成功带与自定义协议入口一致的安全响应头
/// （Content-Security-Policy 按扩展动态生成），失败按状态码回空 body。
fn ext_content_response(
    app: &tauri::AppHandle,
    host: &str,
    rest: &str,
    referer: Option<&str>,
) -> Response<BoxBody<Bytes, Infallible>> {
    // 每个内容请求都记一笔：诊断「iframe 是否真的把请求发到了服务器」的关键证据
    // （白屏排查时：有请求日志 = webview→服务器链路通，问题在渲染/桥；无请求日志
    //  = webview 根本没发请求，问题在 ATS/DNS/webview 配置）。
    let outcome = crate::ext_protocol::serve_http(app, host, rest, referer);
    match &outcome {
        Ok((mime, _, _)) => log::info!("[扩展内容] GET /{rest} host={host} -> 200 {mime}"),
        Err(status) => log::warn!("[扩展内容] GET /{rest} host={host} -> {status}"),
    }
    match outcome {
        Ok((mime, bytes, csp)) => {
            let mut resp = Response::new(BoxBody::new(Full::new(Bytes::from(bytes))));
            *resp.status_mut() = StatusCode::OK;
            let h = resp.headers_mut();
            let _ = h.insert("Content-Type", mime.parse().unwrap());
            let _ = h.insert("Cache-Control", "no-store".parse().unwrap());
            let _ = h.insert("Content-Security-Policy", csp.parse().unwrap());
            let _ = h.insert("X-Content-Type-Options", "nosniff".parse().unwrap());
            let _ = h.insert("Referrer-Policy", "same-origin".parse().unwrap());
            let _ = h.insert("Origin-Agent-Cluster", "?1".parse().unwrap());
            let _ = h.insert(
                "Permissions-Policy",
                "camera=(), microphone=(), geolocation=(), document-domain=()"
                    .parse()
                    .unwrap(),
            );
            resp
        }
        Err(status) => {
            let mut resp =
                Response::new(BoxBody::new(Full::new(Bytes::new())));
            *resp.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::NOT_FOUND);
            let _ = resp
                .headers_mut()
                .insert("Cache-Control", "no-store".parse().unwrap());
            resp
        }
    }
}

async fn forward(app: &tauri::AppHandle, req: Request<Incoming>, ext_id: String, rest: String)
    -> Result<Response<BoxBody<Bytes, Infallible>>, Infallible> {
    // CORS 预检
    if req.method() == Method::OPTIONS {
        let mut resp = Response::new(BoxBody::new(Full::new(Bytes::new())));
        *resp.status_mut() = StatusCode::NO_CONTENT;
        return Ok(resp);
    }

    let port = match service_port(app, &ext_id) {
        Some(p) => p,
        None => {
            return Ok(error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "service not started",
            ))
        }
    };

    let query = req
        .uri()
        .query()
        .map(|q| format!("?{q}"))
        .unwrap_or_default();
    let backend_uri: hyper::Uri = match format!("http://127.0.0.1:{port}{rest}{query}").parse() {
        Ok(u) => u,
        Err(_) => return Ok(error_response(StatusCode::BAD_REQUEST, "bad uri")),
    };

    // WebSocket 升级请求 → 双向隧道（DSH 流式等）
    if is_upgrade(&req) {
        return Ok(handle_upgrade(req, port, &backend_uri).await);
    }

    // 完整读客户端 body（一期非流式）
    let (parts, body) = req.into_parts();
    let body_bytes = match body.collect().await {
        Ok(b) => b.to_bytes(),
        Err(_) => return Ok(error_response(StatusCode::BAD_REQUEST, "bad body")),
    };

    // 连接后端
    let stream = match tokio::net::TcpStream::connect(("127.0.0.1", port)).await {
        Ok(s) => s,
        Err(_) => {
            return Ok(error_response(
                StatusCode::BAD_GATEWAY,
                "backend unreachable",
            ))
        }
    };
    let io = TokioIo::new(stream);
    let (mut sender, conn) = match hyper::client::conn::http1::handshake(io).await {
        Ok(x) => x,
        Err(_) => {
            return Ok(error_response(
                StatusCode::BAD_GATEWAY,
                "backend handshake failed",
            ))
        }
    };
    tokio::spawn(async move {
        let _ = conn.await;
    });

    // 构造后端请求：透传 headers（剔除 hop-by-hop）
    let mut builder = backend_request_builder(parts.method, &backend_uri);
    for (k, v) in parts.headers.iter() {
        if is_hop_by_hop(k.as_str()) {
            continue;
        }
        builder = builder.header(k, v);
    }
    let backend_req = match builder.body(Full::new(body_bytes)) {
        Ok(r) => r,
        Err(_) => return Ok(error_response(StatusCode::BAD_REQUEST, "bad request")),
    };

    let resp = match sender.send_request(backend_req).await {
        Ok(r) => r,
        Err(_) => return Ok(error_response(StatusCode::BAD_GATEWAY, "backend error")),
    };
    let (rparts, rbody) = resp.into_parts();
    let rbytes = match rbody.collect().await {
        Ok(b) => b.to_bytes(),
        Err(_) => {
            return Ok(error_response(
                StatusCode::BAD_GATEWAY,
                "backend body error",
            ))
        }
    };
    let mut out = Response::new(BoxBody::new(Full::new(rbytes)));
    *out.status_mut() = rparts.status;
    *out.headers_mut() = rparts.headers;
    Ok(out)
}

/// 是否为 WebSocket 升级请求（Upgrade 头 + Connection: upgrade）
fn is_upgrade(req: &Request<Incoming>) -> bool {
    req.headers().get(UPGRADE).is_some()
        && req
            .headers()
            .get(CONNECTION)
            .map(|v| v.to_str().unwrap_or("").to_ascii_lowercase().contains("upgrade"))
            .unwrap_or(false)
}

/// WebSocket 升级反向代理：客户端 ↔ 后端各建升级连接，双向 copy 隧道。
/// 返回客户端侧的 101 Switching Protocols 响应。
async fn handle_upgrade(
    mut req: Request<Incoming>,
    port: u16,
    backend_uri: &hyper::Uri,
) -> Response<BoxBody<Bytes, Infallible>> {
    // 客户端侧升级句柄（先登记，后 await 拿 Upgraded 流）
    let client_on_upgrade = hyper::upgrade::on(&mut req);

    // 连接后端
    let stream = match tokio::net::TcpStream::connect(("127.0.0.1", port)).await {
        Ok(s) => s,
        Err(_) => return error_response(StatusCode::BAD_GATEWAY, "backend unreachable"),
    };
    let io = TokioIo::new(stream);
    let (mut sender, conn) = match hyper::client::conn::http1::handshake(io).await {
        Ok(x) => x,
        Err(_) => return error_response(StatusCode::BAD_GATEWAY, "backend handshake failed"),
    };
    tokio::spawn(async move {
        let _ = conn.with_upgrades().await;
    });

    // 构造后端请求：保留 upgrade/connection，Host 按可信后端地址重建。
    let (parts, _body) = req.into_parts();
    let mut builder = backend_request_builder(parts.method, backend_uri);
    for (k, v) in parts.headers.iter() {
        if k.as_str().to_ascii_lowercase() == "host" {
            continue;
        }
        builder = builder.header(k, v);
    }
    let backend_req = match builder.body(Full::new(Bytes::new())) {
        Ok(r) => r,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "bad request"),
    };

    let mut backend_resp = match sender.send_request(backend_req).await {
        Ok(r) => r,
        Err(_) => return error_response(StatusCode::BAD_GATEWAY, "backend error"),
    };
    if backend_resp.status() != StatusCode::SWITCHING_PROTOCOLS {
        return error_response(StatusCode::BAD_GATEWAY, "backend not switching protocols");
    }
    let backend_on_upgrade = hyper::upgrade::on(&mut backend_resp);

    // 客户端 101 响应
    let mut resp = match Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header(UPGRADE, "websocket")
        .header(CONNECTION, "upgrade")
        .body(BoxBody::new(Full::new(Bytes::new())))
    {
        Ok(r) => r,
        Err(_) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, "bad upgrade response"),
    };

    // 保留 WebSocket 握手验签头与子协议，否则浏览器会拒绝 101。
    for name in ["sec-websocket-accept", "sec-websocket-protocol", "sec-websocket-extensions"] {
        if let Some(value) = backend_resp.headers().get(name) { resp.headers_mut().insert(name, value.clone()); }
    }

    // 双向隧道：两端 Upgraded 流互相转发
    tokio::spawn(async move {
        let client = match client_on_upgrade.await {
            Ok(u) => u,
            Err(e) => {
                log::debug!("客户端升级失败: {e}");
                return;
            }
        };
        let backend = match backend_on_upgrade.await {
            Ok(u) => u,
            Err(e) => {
                log::debug!("后端升级失败: {e}");
                return;
            }
        };
        // TokioIo 包装：Upgraded 实现的是 hyper 的 Read/Write trait，转成 tokio 的 AsyncRead/AsyncWrite
        let (mut client, mut backend) = (TokioIo::new(client), TokioIo::new(backend));
        if let Err(e) = tokio::io::copy_bidirectional(&mut client, &mut backend).await {
            log::debug!("WebSocket 隧道结束: {e}");
        }
    });

    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_forward_request_preserves_path_query_and_backend_host() {
        let uri = "http://127.0.0.1:34567/health?check=1".parse().unwrap();
        let request = backend_request_builder(Method::GET, &uri).body(()).unwrap();
        assert_eq!(request.uri(), "/health?check=1");
        assert_eq!(request.headers()["host"], "127.0.0.1:34567");
    }

    #[test]
    fn security_proxy_requires_token_and_extension_origin() {
        let state = ProxyState::new(9999);
        let prefix = state.prefix("demo").unwrap();
        let valid = Request::builder().uri(format!("{prefix}/api/data"))
            .header("origin", crate::ext_protocol::origin("demo")).body(()).unwrap();
        assert_eq!(state.authorize(&valid), Some(("demo".into(), "/api/data".into())));
        let bad = Request::builder().uri(format!("{prefix}/api/data"))
            .header("origin", "https://example.invalid").body(()).unwrap();
        assert!(state.authorize(&bad).is_none());
        let no_token = Request::builder().uri("/svc/demo/api/data")
            .header("origin", crate::ext_protocol::origin("demo")).body(()).unwrap();
        assert!(state.authorize(&no_token).is_none());
        let no_origin = Request::builder().uri(prefix).body(()).unwrap();
        assert!(state.authorize(&no_origin).is_none());
    }

    #[test]
    fn parse_proxy_path_splits_ext_id_and_rest() {
        assert_eq!(
            parse_proxy_path("/svc/com.x-hub.hello/api/hello"),
            Some(("com.x-hub.hello".to_string(), "/api/hello".to_string()))
        );
        assert_eq!(
            parse_proxy_path("/svc/com.x-hub.hello"),
            Some(("com.x-hub.hello".to_string(), "/".to_string()))
        );
        assert_eq!(parse_proxy_path("/other"), None);
        assert_eq!(parse_proxy_path("/svc/"), None);
    }

    #[test]
    fn hop_by_hop_headers_are_filtered() {
        assert!(is_hop_by_hop("Connection"));
        assert!(is_hop_by_hop("host"));
        assert!(is_hop_by_hop("Upgrade"));
        assert!(!is_hop_by_hop("content-type"));
        assert!(!is_hop_by_hop("authorization"));
    }
}
