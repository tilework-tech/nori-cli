//! Real MCP calls used by delegation integration scenarios.

use nori_protocol::acp::v1 as acp;
use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;

pub(crate) fn advertised(servers: &[acp::McpServer]) -> Option<acp::McpServerHttp> {
    servers.iter().find_map(|server| match server {
        acp::McpServer::Http(http) if http.name == "nori-client" => Some(http.clone()),
        _ => None,
    })
}

pub(crate) async fn call(server: Option<acp::McpServerHttp>, request: &str) -> String {
    match call_inner(server, request).await {
        Ok(text) => text,
        Err(error) => format!("Delegation tool error: {error}"),
    }
}

async fn call_inner(
    server: Option<acp::McpServerHttp>,
    request: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let server = server.ok_or("nori-client MCP unavailable")?;
    let (agent, task) = request.split_once(' ').ok_or("expected agent and task")?;
    let bearer = server
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("authorization"))
        .and_then(|header| header.value.strip_prefix("Bearer "))
        .ok_or("missing MCP authorization")?;
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(server.url).auth_header(bearer.to_owned()),
    );
    let client = ().serve(transport).await?;
    let result = client
        .call_tool(
            CallToolRequestParams::new("delegate").with_arguments(
                serde_json::json!({"agent":agent,"task":task})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await?;
    let text = result
        .content
        .iter()
        .filter_map(|content| content.as_text())
        .map(|text| text.text.clone())
        .collect::<Vec<_>>()
        .join("\n");
    client.cancel().await?;
    Ok(text)
}
