//! Conversion from CLI MCP server config to SACP protocol types.
//!
//! The CLI stores MCP server configuration in `codex_core::config::types::McpServerConfig`.
//! The SACP protocol expects `sacp::schema::McpServer` in `NewSessionRequest`.
//! This module bridges the two so that CLI-configured MCP servers are forwarded
//! to ACP agents at session creation time.

use std::collections::HashMap;

use codex_core::config::types::McpServerConfig;
use codex_core::config::types::McpServerTransportConfig;
use sacp::schema as acp;
use tracing::warn;

/// Convert CLI MCP server configs into SACP protocol `McpServer` values
/// suitable for inclusion in a `NewSessionRequest`.
///
/// All servers are included regardless of their `enabled` flag — the agent
/// decides how to handle them.
///
/// Environment variable references (`bearer_token_env_var`, `env_http_headers`,
/// `env_vars`) are resolved eagerly from the current process environment.
/// Missing variables are logged as warnings and skipped.
pub fn to_sacp_mcp_servers(servers: &HashMap<String, McpServerConfig>) -> Vec<acp::McpServer> {
    let mut result: Vec<acp::McpServer> = servers
        .iter()
        .map(|(name, config)| convert_one(name, config))
        .collect();
    // Sort for deterministic ordering (HashMap iteration is random).
    result.sort_by(|a, b| mcp_server_name(a).cmp(mcp_server_name(b)));
    result
}

fn mcp_server_name(server: &acp::McpServer) -> &str {
    match server {
        acp::McpServer::Http(s) => &s.name,
        acp::McpServer::Sse(s) => &s.name,
        acp::McpServer::Stdio(s) => &s.name,
        _ => "",
    }
}

fn convert_one(name: &str, config: &McpServerConfig) -> acp::McpServer {
    match &config.transport {
        McpServerTransportConfig::Stdio {
            command,
            args,
            env,
            env_vars,
        } => {
            let mut env_list: Vec<acp::EnvVariable> = Vec::new();

            // Explicit key-value env vars.
            if let Some(env_map) = env {
                for (k, v) in env_map {
                    env_list.push(acp::EnvVariable::new(k.clone(), v.clone()));
                }
            }

            // Env vars inherited from the current process by name.
            for var_name in env_vars {
                match std::env::var(var_name) {
                    Ok(val) => env_list.push(acp::EnvVariable::new(var_name.clone(), val)),
                    Err(_) => {
                        warn!(
                            "MCP server '{name}': env var '{var_name}' not found in environment, skipping"
                        );
                    }
                }
            }

            acp::McpServer::Stdio(
                acp::McpServerStdio::new(name, command.as_str())
                    .args(args.clone())
                    .env(env_list),
            )
        }
        McpServerTransportConfig::StreamableHttp {
            url,
            bearer_token_env_var,
            http_headers,
            env_http_headers,
        } => {
            let mut headers: Vec<acp::HttpHeader> = Vec::new();

            // Static headers.
            if let Some(header_map) = http_headers {
                for (k, v) in header_map {
                    headers.push(acp::HttpHeader::new(k.clone(), v.clone()));
                }
            }

            // Headers whose values come from environment variables.
            if let Some(env_header_map) = env_http_headers {
                for (header_name, env_var_name) in env_header_map {
                    match std::env::var(env_var_name) {
                        Ok(val) => headers.push(acp::HttpHeader::new(header_name.clone(), val)),
                        Err(_) => {
                            warn!(
                                "MCP server '{name}': env var '{env_var_name}' for header '{header_name}' not found, skipping"
                            );
                        }
                    }
                }
            }

            // Bearer token from env var → Authorization header.
            if let Some(token_env_var) = bearer_token_env_var {
                match std::env::var(token_env_var) {
                    Ok(token) => {
                        headers.push(acp::HttpHeader::new(
                            "Authorization".to_string(),
                            format!("Bearer {token}"),
                        ));
                    }
                    Err(_) => {
                        warn!(
                            "MCP server '{name}': bearer token env var '{token_env_var}' not found, skipping auth header"
                        );
                    }
                }
            }

            acp::McpServer::Http(acp::McpServerHttp::new(name, url.as_str()).headers(headers))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn stdio_config(
        command: &str,
        args: Vec<&str>,
        env: Option<HashMap<String, String>>,
        env_vars: Vec<String>,
    ) -> McpServerConfig {
        McpServerConfig {
            transport: McpServerTransportConfig::Stdio {
                command: command.to_string(),
                args: args.into_iter().map(String::from).collect(),
                env,
                env_vars,
            },
            enabled: true,
            startup_timeout_sec: None,
            tool_timeout_sec: None,
            enabled_tools: None,
            disabled_tools: None,
        }
    }

    fn http_config(
        url: &str,
        bearer_token_env_var: Option<String>,
        http_headers: Option<HashMap<String, String>>,
        env_http_headers: Option<HashMap<String, String>>,
    ) -> McpServerConfig {
        McpServerConfig {
            transport: McpServerTransportConfig::StreamableHttp {
                url: url.to_string(),
                bearer_token_env_var,
                http_headers,
                env_http_headers,
            },
            enabled: true,
            startup_timeout_sec: None,
            tool_timeout_sec: None,
            enabled_tools: None,
            disabled_tools: None,
        }
    }

    #[test]
    fn empty_input_produces_empty_output() {
        let servers = HashMap::new();
        let result = to_sacp_mcp_servers(&servers);
        assert!(result.is_empty());
    }

    #[test]
    fn stdio_server_is_converted() {
        let mut servers = HashMap::new();
        let mut env = HashMap::new();
        env.insert("API_KEY".to_string(), "secret".to_string());

        servers.insert(
            "my-server".to_string(),
            stdio_config("npx", vec!["@mcp/server", "/tmp"], Some(env), vec![]),
        );

        let result = to_sacp_mcp_servers(&servers);
        assert_eq!(result.len(), 1);

        match &result[0] {
            acp::McpServer::Stdio(s) => {
                assert_eq!(s.name, "my-server");
                assert_eq!(s.command.to_str().unwrap(), "npx");
                assert_eq!(s.args, vec!["@mcp/server", "/tmp"]);
                assert_eq!(s.env.len(), 1);
                assert_eq!(s.env[0].name, "API_KEY");
                assert_eq!(s.env[0].value, "secret");
            }
            other => panic!("Expected Stdio, got {other:?}"),
        }
    }

    #[test]
    fn http_server_with_bearer_token_is_converted() {
        // Set the env var for the test.
        unsafe { std::env::set_var("TEST_MCP_TOKEN", "my-secret-token") };

        let mut servers = HashMap::new();
        servers.insert(
            "remote".to_string(),
            http_config(
                "https://mcp.example.com",
                Some("TEST_MCP_TOKEN".to_string()),
                None,
                None,
            ),
        );

        let result = to_sacp_mcp_servers(&servers);
        assert_eq!(result.len(), 1);

        match &result[0] {
            acp::McpServer::Http(s) => {
                assert_eq!(s.name, "remote");
                assert_eq!(s.url, "https://mcp.example.com");
                assert_eq!(s.headers.len(), 1);
                assert_eq!(s.headers[0].name, "Authorization");
                assert_eq!(s.headers[0].value, "Bearer my-secret-token");
            }
            other => panic!("Expected Http, got {other:?}"),
        }

        unsafe { std::env::remove_var("TEST_MCP_TOKEN") };
    }

    #[test]
    fn http_server_with_static_and_env_headers() {
        unsafe { std::env::set_var("TEST_MCP_HEADER_VAL", "env-header-value") };

        let mut static_headers = HashMap::new();
        static_headers.insert("X-Static".to_string(), "static-value".to_string());

        let mut env_headers = HashMap::new();
        env_headers.insert("X-Dynamic".to_string(), "TEST_MCP_HEADER_VAL".to_string());

        let mut servers = HashMap::new();
        servers.insert(
            "headers-server".to_string(),
            http_config(
                "https://mcp.example.com",
                None,
                Some(static_headers),
                Some(env_headers),
            ),
        );

        let result = to_sacp_mcp_servers(&servers);
        assert_eq!(result.len(), 1);

        match &result[0] {
            acp::McpServer::Http(s) => {
                assert_eq!(s.headers.len(), 2);
                let header_names: Vec<&str> = s.headers.iter().map(|h| h.name.as_str()).collect();
                assert!(header_names.contains(&"X-Static"));
                assert!(header_names.contains(&"X-Dynamic"));

                let dynamic = s.headers.iter().find(|h| h.name == "X-Dynamic").unwrap();
                assert_eq!(dynamic.value, "env-header-value");
            }
            other => panic!("Expected Http, got {other:?}"),
        }

        unsafe { std::env::remove_var("TEST_MCP_HEADER_VAL") };
    }

    #[test]
    fn missing_env_var_is_skipped_not_error() {
        let mut servers = HashMap::new();
        servers.insert(
            "broken".to_string(),
            http_config(
                "https://mcp.example.com",
                Some("NONEXISTENT_TOKEN_VAR_12345".to_string()),
                None,
                None,
            ),
        );

        let result = to_sacp_mcp_servers(&servers);
        assert_eq!(result.len(), 1);

        match &result[0] {
            acp::McpServer::Http(s) => {
                // No Authorization header because the env var was missing.
                assert!(s.headers.is_empty());
            }
            other => panic!("Expected Http, got {other:?}"),
        }
    }

    #[test]
    fn disabled_servers_are_included() {
        let mut servers = HashMap::new();
        servers.insert(
            "disabled-server".to_string(),
            McpServerConfig {
                transport: McpServerTransportConfig::Stdio {
                    command: "echo".to_string(),
                    args: vec![],
                    env: None,
                    env_vars: vec![],
                },
                enabled: false,
                startup_timeout_sec: None,
                tool_timeout_sec: None,
                enabled_tools: None,
                disabled_tools: None,
            },
        );

        let result = to_sacp_mcp_servers(&servers);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn env_vars_are_resolved_from_process_env() {
        unsafe { std::env::set_var("TEST_MCP_INHERITED_VAR", "inherited-value") };

        let mut servers = HashMap::new();
        servers.insert(
            "env-server".to_string(),
            stdio_config(
                "my-cmd",
                vec![],
                None,
                vec!["TEST_MCP_INHERITED_VAR".to_string()],
            ),
        );

        let result = to_sacp_mcp_servers(&servers);
        match &result[0] {
            acp::McpServer::Stdio(s) => {
                assert_eq!(s.env.len(), 1);
                assert_eq!(s.env[0].name, "TEST_MCP_INHERITED_VAR");
                assert_eq!(s.env[0].value, "inherited-value");
            }
            other => panic!("Expected Stdio, got {other:?}"),
        }

        unsafe { std::env::remove_var("TEST_MCP_INHERITED_VAR") };
    }

    #[test]
    fn multiple_servers_are_sorted_by_name() {
        let mut servers = HashMap::new();
        servers.insert("zebra".to_string(), stdio_config("z", vec![], None, vec![]));
        servers.insert("alpha".to_string(), stdio_config("a", vec![], None, vec![]));
        servers.insert(
            "middle".to_string(),
            http_config("https://m.example.com", None, None, None),
        );

        let result = to_sacp_mcp_servers(&servers);
        assert_eq!(result.len(), 3);

        let names: Vec<&str> = result
            .iter()
            .map(|s| match s {
                acp::McpServer::Http(h) => h.name.as_str(),
                acp::McpServer::Sse(h) => h.name.as_str(),
                acp::McpServer::Stdio(h) => h.name.as_str(),
                _ => "",
            })
            .collect();
        assert_eq!(names, vec!["alpha", "middle", "zebra"]);
    }
}
