use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::http::StatusCode;
use axum::routing::post;
use codex_rmcp_client::OAuthCredentialsStoreMode;
use codex_rmcp_client::RmcpClient;
use futures::FutureExt;
use mcp_types::InitializeRequestParams;
use pretty_assertions::assert_eq;
use rmcp::ServerHandler;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::model::ServerCapabilities;
use rmcp::model::ServerConfig;
use rmcp::schemars;
use rmcp::tool;
use rmcp::tool_handler;
use rmcp::tool_router;
use rmcp::transport::StreamableHttpService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use serde::Deserialize;
use serde_json::json;

const TIMEOUT: Option<Duration> = Some(Duration::from_secs(10));

async fn initialize(client: &RmcpClient) -> anyhow::Result<String> {
    let params: InitializeRequestParams = serde_json::from_value(json!({
        "protocolVersion": "2025-06-18",
        "capabilities": {},
        "clientInfo": { "name": "nori-transport-test", "version": "1" }
    }))?;
    let result = client
        .initialize(
            params,
            TIMEOUT,
            Box::new(|_, _| async { anyhow::bail!("unexpected elicitation") }.boxed()),
        )
        .await?;
    Ok(result.protocol_version)
}

async fn round_trip(client: &RmcpClient, protocol_version: &str) -> anyhow::Result<()> {
    assert_eq!(initialize(client).await?, protocol_version);
    let tools = client.list_tools(None, TIMEOUT).await?;
    assert!(tools.tools.iter().any(|tool| tool.name == "echo"));
    let result = client
        .call_tool(
            "echo".into(),
            Some(json!({"message": "hello MCP"})),
            TIMEOUT,
        )
        .await?;
    assert_eq!(result.is_error, Some(false));
    assert_eq!(
        result.structured_content,
        Some(json!({"echo": "ECHOING: hello MCP", "env": null}))
    );
    Ok(())
}

#[tokio::test]
async fn stdio_lists_and_calls_tools() -> anyhow::Result<()> {
    let client = RmcpClient::new_stdio_client(
        env!("CARGO_BIN_EXE_test_stdio_server").into(),
        vec![],
        None,
        &[],
        None,
    )
    .await?;
    round_trip(&client, "2025-03-26").await?;
    println!("stdio: initialize (2025-03-26), tools/list, tools/call echo => ECHOING: hello MCP");
    Ok(())
}

#[derive(Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct EchoArgs {
    message: String,
}

#[derive(Clone)]
struct EchoServer {
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl EchoServer {
    #[tool(description = "Echo the message")]
    async fn echo(
        &self,
        Parameters(args): Parameters<EchoArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        Ok(CallToolResult::structured(
            json!({"echo": format!("ECHOING: {}", args.message), "env": null}),
        ))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for EchoServer {
    fn supported_protocol_versions(
        &self,
    ) -> std::borrow::Cow<'static, [rmcp::model::ProtocolVersion]> {
        std::borrow::Cow::Borrowed(rmcp::model::ProtocolVersion::known_up_to(
            &rmcp::model::ProtocolVersion::V_2025_06_18,
        ))
    }

    fn get_info(&self) -> ServerConfig {
        let mut info = ServerConfig::default();
        info.protocol_version = rmcp::model::ProtocolVersion::V_2025_06_18;
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info
    }
}

struct HttpServer {
    url: String,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

impl HttpServer {
    async fn start(router: Router) -> anyhow::Result<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}/mcp", listener.local_addr()?);
        let task = tokio::spawn(async move { axum::serve(listener, router).await });
        Ok(Self { url, task })
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::test]
async fn streamable_http_lists_and_calls_tools_with_auth_and_headers() -> anyhow::Result<()> {
    let service = StreamableHttpService::new(
        || {
            Ok(EchoServer {
                tool_router: EchoServer::tool_router(),
            })
        },
        Arc::new(LocalSessionManager::default()),
        Default::default(),
    );
    let router = Router::new()
        .nest_service("/mcp", service)
        .layer(axum::middleware::from_fn(
            |request: axum::extract::Request, next: axum::middleware::Next| async move {
                if request
                    .headers()
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                    != Some("Bearer test-token")
                    || request
                        .headers()
                        .get("x-nori-test")
                        .and_then(|v| v.to_str().ok())
                        != Some("custom-header")
                {
                    return Err(StatusCode::UNAUTHORIZED);
                }
                Ok(next.run(request).await)
            },
        ));
    let server = HttpServer::start(router).await?;
    let client = RmcpClient::new_streamable_http_client(
        "transport-test",
        &server.url,
        Some("test-token".into()),
        Some(HashMap::from([(
            "x-nori-test".into(),
            "custom-header".into(),
        )])),
        None,
        OAuthCredentialsStoreMode::File,
    )
    .await?;
    round_trip(&client, "2025-06-18").await?;
    println!(
        "streamable HTTP: bearer + custom header, initialize (2025-06-18), tools/list, tools/call echo => ECHOING: hello MCP"
    );
    Ok(())
}

#[tokio::test]
async fn streamable_http_rejects_oversized_response_bodies() -> anyhow::Result<()> {
    for (status, size) in [
        (StatusCode::BAD_REQUEST, 64 * 1024),
        (StatusCode::OK, 16 * 1024 * 1024),
    ] {
        for chunked in [false, true] {
            let router = Router::new().route(
                "/mcp",
                post(move || async move {
                    let body = if chunked {
                        axum::body::Body::from_stream(futures::stream::iter(
                            (0..=size / 1024).map(|_| Ok::<_, std::io::Error>(" ".repeat(1024))),
                        ))
                    } else {
                        axum::body::Body::from(" ".repeat(size + 1))
                    };
                    (status, [("content-type", "application/json")], body)
                }),
            );
            let server = HttpServer::start(router).await?;
            let client = RmcpClient::new_streamable_http_client(
                "oversized-response-test",
                &server.url,
                Some("test-token".into()),
                None,
                None,
                OAuthCredentialsStoreMode::File,
            )
            .await?;
            let error = initialize(&client)
                .await
                .expect_err("oversized response must fail");
            let message = error.to_string();
            assert!(
                message.contains(&format!(
                    "HTTP response body exceeded the maximum size of {size} bytes"
                )),
                "unexpected failure: {message}"
            );
            println!(
                "streamable HTTP: {status} response over {size} bytes rejected before parsing (chunked={chunked})"
            );
        }
    }
    Ok(())
}
