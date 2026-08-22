//! `/svc/<extId>/*` 反向代理（spec §5 代理转发）。
//!
//! 宿主启动一个监听 `127.0.0.1` 动态端口的 HTTP 服务器，把 `/svc/<extId>/<rest>` 转发到
//! 对应 service 扩展后端 `127.0.0.1:<port>/<rest>`，并在响应统一加 CORS 头（Allow-Origin: *），
//! 让 asset origin 的扩展 iframe 能直接 `fetch` / 原生 `WebSocket` 访问后端而无需后端自行处理 CORS。
//!
//! 一期范围：非流式 HTTP 转发（完整读 body）。WebSocket 升级（DSH 流式 `/api/events.mux`）后续补。

use crate::service::service_port;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::convert::Infallible;

/// 代理服务器端口（0 = 未启动）
pub struct ProxyState(pub u16);

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
                .await
            {
                // 连接结束/客户端断开属正常，debug 记录避免噪音
                log::debug!("代理连接结束: {e}");
            }
        });
    }
}

fn add_cors(resp: &mut Response<BoxBody<Bytes, Infallible>>) {
    let h = resp.headers_mut();
    let _ = h.insert("Access-Control-Allow-Origin", "*".parse().unwrap());
    let _ = h.insert("Access-Control-Allow-Methods", "*".parse().unwrap());
    let _ = h.insert("Access-Control-Allow-Headers", "*".parse().unwrap());
}

fn error_response(status: StatusCode, msg: &str) -> Response<BoxBody<Bytes, Infallible>> {
    let mut resp = Response::new(BoxBody::new(Full::new(Bytes::from(msg.to_string()))));
    *resp.status_mut() = status;
    add_cors(&mut resp);
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

/// hop-by-hop 头（转发时剔除；hyper 客户端会按 URI 自动重建 Host）
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

async fn handle(
    app: &tauri::AppHandle,
    req: Request<Incoming>,
) -> Result<Response<BoxBody<Bytes, Infallible>>, Infallible> {
    // CORS 预检
    if req.method() == Method::OPTIONS {
        let mut resp = Response::new(BoxBody::new(Full::new(Bytes::new())));
        *resp.status_mut() = StatusCode::NO_CONTENT;
        add_cors(&mut resp);
        return Ok(resp);
    }

    let (ext_id, rest) = match parse_proxy_path(req.uri().path()) {
        Some(x) => x,
        None => return Ok(error_response(StatusCode::NOT_FOUND, "not found")),
    };
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
    let mut builder = Request::builder().method(parts.method).uri(backend_uri);
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
    add_cors(&mut out);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

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
