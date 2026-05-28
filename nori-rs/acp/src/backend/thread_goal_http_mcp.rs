use anyhow::Context;
use anyhow::Result;
use serde_json::Value;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::net::TcpStream;

use super::thread_goal_mcp::ThreadGoalMcpBridge;

pub(crate) struct GoalMcpHttpServer {
    url: String,
    abort_handle: tokio::task::AbortHandle,
}

impl GoalMcpHttpServer {
    pub(crate) async fn spawn(bridge: ThreadGoalMcpBridge) -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .context("failed to bind local goal MCP HTTP server")?;
        let url = format!("http://{}/mcp", listener.local_addr()?);
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _addr)) = listener.accept().await else {
                    break;
                };
                let bridge = bridge.clone();
                tokio::spawn(async move {
                    if let Err(err) = handle_connection(stream, bridge).await {
                        tracing::debug!("local goal MCP HTTP request failed: {err}");
                    }
                });
            }
        });

        Ok(Self {
            url,
            abort_handle: task.abort_handle(),
        })
    }

    pub(crate) fn url(&self) -> &str {
        &self.url
    }
}

impl Drop for GoalMcpHttpServer {
    fn drop(&mut self) {
        self.abort_handle.abort();
    }
}

async fn handle_connection(mut stream: TcpStream, bridge: ThreadGoalMcpBridge) -> Result<()> {
    let request = read_http_request(&mut stream).await?;
    let (status, body) = match request {
        HttpRequest::Post { body } => match serde_json::from_slice::<Value>(&body) {
            Ok(value) => handle_json_rpc(bridge, value).await,
            Err(err) => (
                "400 Bad Request",
                Some(json_rpc_error(
                    Value::Null,
                    -32700,
                    format!("parse error: {err}"),
                )),
            ),
        },
        HttpRequest::Options => ("204 No Content", None),
        HttpRequest::Other => (
            "405 Method Not Allowed",
            Some(serde_json::json!({ "error": "method not allowed" })),
        ),
    };
    write_http_response(&mut stream, status, body.as_ref()).await
}

enum HttpRequest {
    Post { body: Vec<u8> },
    Options,
    Other,
}

async fn read_http_request(stream: &mut TcpStream) -> Result<HttpRequest> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    let header_end = loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            anyhow::bail!("connection closed before HTTP headers");
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(header_end) = find_header_end(&buffer) {
            break header_end;
        }
    };

    let headers = String::from_utf8_lossy(&buffer[..header_end]);
    let request_line = headers.lines().next().unwrap_or_default();
    if request_line.starts_with("OPTIONS ") {
        return Ok(HttpRequest::Options);
    }
    if !request_line.starts_with("POST ") {
        return Ok(HttpRequest::Other);
    }

    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    let body_start = header_end + 4;
    while buffer.len().saturating_sub(body_start) < content_length {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            anyhow::bail!("connection closed before HTTP body");
        }
        buffer.extend_from_slice(&chunk[..read]);
    }

    Ok(HttpRequest::Post {
        body: buffer[body_start..body_start + content_length].to_vec(),
    })
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

async fn handle_json_rpc(
    bridge: ThreadGoalMcpBridge,
    value: Value,
) -> (&'static str, Option<Value>) {
    if let Some(items) = value.as_array() {
        let mut responses = Vec::new();
        for item in items {
            if let Some(response) = handle_json_rpc_message(&bridge, item.clone()).await {
                responses.push(response);
            }
        }
        return if responses.is_empty() {
            ("202 Accepted", None)
        } else {
            ("200 OK", Some(Value::Array(responses)))
        };
    }

    match handle_json_rpc_message(&bridge, value).await {
        Some(response) => ("200 OK", Some(response)),
        None => ("202 Accepted", None),
    }
}

async fn handle_json_rpc_message(bridge: &ThreadGoalMcpBridge, message: Value) -> Option<Value> {
    let id = message.get("id").cloned();
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = message
        .get("params")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));

    let id = id?;
    let result = bridge.handle_mcp_request(method, params).await;
    Some(serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    }))
}

fn json_rpc_error(id: Value, code: i64, message: String) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message,
        },
    })
}

async fn write_http_response(
    stream: &mut TcpStream,
    status: &str,
    body: Option<&Value>,
) -> Result<()> {
    let body = body.map(serde_json::to_vec).transpose()?;
    let body_len = body.as_ref().map_or(0, Vec::len);
    let content_type = if body.is_some() {
        "application/json"
    } else {
        "text/plain"
    };
    let headers = format!(
        "HTTP/1.1 {status}\r\n\
Access-Control-Allow-Origin: *\r\n\
Access-Control-Allow-Headers: content-type, mcp-session-id, mcp-protocol-version\r\n\
Access-Control-Allow-Methods: POST, OPTIONS\r\n\
Content-Type: {content_type}\r\n\
Content-Length: {body_len}\r\n\
Connection: close\r\n\r\n"
    );
    stream.write_all(headers.as_bytes()).await?;
    if let Some(body) = body {
        stream.write_all(&body).await?;
    }
    Ok(())
}
