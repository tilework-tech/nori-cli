//! Conversion from CLI MCP server config to SACP protocol types.
//!
//! The CLI stores MCP server configuration in `codex_core::config::types::McpServerConfig`.
//! The SACP protocol expects `sacp::schema::McpServer` in `NewSessionRequest`.
//! This module bridges the two so that CLI-configured MCP servers are forwarded
//! to ACP agents at session creation time.

use std::collections::HashMap;

use codex_core::config::types::McpServerConfig;
use codex_core::config::types::McpServerTransportConfig;
use codex_rmcp_client::OAuthCredentialsStoreMode;
use codex_rmcp_client::load_oauth_tokens;
use oauth2::TokenResponse;
use sacp::schema as acp;
use tracing::warn;

/// Convert CLI MCP server configs into SACP protocol `McpServer` values
/// suitable for inclusion in a `NewSessionRequest`.
///
/// Disabled servers (`enabled == false`) are excluded.
///
/// Environment variable references (`bearer_token_env_var`, `env_http_headers`,
/// `env_vars`) are resolved eagerly from the current process environment.
/// Missing variables are logged as warnings and skipped.
///
/// For HTTP servers without a `bearer_token_env_var`, stored OAuth tokens
/// (from keyring or credential file) are loaded and injected as an
/// `Authorization: Bearer` header.
pub fn to_sacp_mcp_servers(servers: &HashMap<String, McpServerConfig>) -> Vec<acp::McpServer> {
    let mut result: Vec<acp::McpServer> = servers
        .iter()
        .filter(|(_, config)| config.enabled)
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
            ..
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
            let mut has_bearer = false;
            if let Some(token_env_var) = bearer_token_env_var {
                match std::env::var(token_env_var) {
                    Ok(token) => {
                        headers.push(acp::HttpHeader::new(
                            "Authorization".to_string(),
                            format!("Bearer {token}"),
                        ));
                        has_bearer = true;
                    }
                    Err(_) => {
                        warn!(
                            "MCP server '{name}': bearer token env var '{token_env_var}' not found, skipping auth header"
                        );
                    }
                }
            }

            // Fall back to stored OAuth tokens if no bearer token env var was resolved.
            if !has_bearer {
                match load_oauth_tokens(name, url, OAuthCredentialsStoreMode::Auto) {
                    Ok(Some(tokens)) => {
                        if let Some(expires_at) = tokens.expires_at {
                            let now_ms = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as u64;
                            if expires_at <= now_ms {
                                warn!(
                                    "MCP server '{name}': stored OAuth token has expired; \
                                     re-authenticate via /mcp"
                                );
                            }
                        }
                        let access_token = tokens.token_response.0.access_token().secret();
                        headers.push(acp::HttpHeader::new(
                            "Authorization".to_string(),
                            format!("Bearer {access_token}"),
                        ));
                    }
                    Ok(None) => {}
                    Err(e) => {
                        warn!("MCP server '{name}': failed to load stored OAuth tokens: {e}");
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
    use serial_test::serial;

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
                client_id: None,
                client_secret_env_var: None,
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
    fn disabled_servers_are_excluded() {
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
        servers.insert(
            "enabled-server".to_string(),
            stdio_config("echo", vec![], None, vec![]),
        );

        let result = to_sacp_mcp_servers(&servers);
        assert_eq!(result.len(), 1);
        match &result[0] {
            acp::McpServer::Stdio(s) => {
                assert_eq!(s.name, "enabled-server");
            }
            other => panic!("Expected Stdio, got {other:?}"),
        }
    }

    #[test]
    #[serial]
    fn http_server_with_stored_oauth_tokens_gets_auth_header() {
        use codex_rmcp_client::OAuthCredentialsStoreMode;
        use codex_rmcp_client::StoredOAuthTokens;
        use codex_rmcp_client::WrappedOAuthTokenResponse;
        use codex_rmcp_client::save_oauth_tokens;
        use oauth2::AccessToken;
        use oauth2::EmptyExtraTokenFields;
        use oauth2::basic::BasicTokenType;
        use rmcp::transport::auth::OAuthTokenResponse;

        let tmp_dir = tempfile::tempdir().unwrap();
        let tmp_path = tmp_dir.path().to_str().unwrap().to_string();

        // Point CODEX_HOME at temp dir so file-based credential storage works
        let old_codex_home = std::env::var("CODEX_HOME").ok();
        unsafe { std::env::set_var("CODEX_HOME", &tmp_path) };

        let server_name = "linear";
        let server_url = "https://mcp.linear.app/mcp";

        // Store OAuth tokens for this server
        let token_response = OAuthTokenResponse::new(
            AccessToken::new("test-oauth-access-token".to_string()),
            BasicTokenType::Bearer,
            EmptyExtraTokenFields {},
        );
        let stored = StoredOAuthTokens {
            server_name: server_name.to_string(),
            url: server_url.to_string(),
            client_id: "test-client-id".to_string(),
            token_response: WrappedOAuthTokenResponse(token_response),
            expires_at: None,
        };
        save_oauth_tokens(server_name, &stored, OAuthCredentialsStoreMode::File).unwrap();

        // Create an HTTP server config without bearer_token_env_var
        let mut servers = HashMap::new();
        servers.insert(
            server_name.to_string(),
            http_config(server_url, None, None, None),
        );

        let result = to_sacp_mcp_servers(&servers);
        assert_eq!(result.len(), 1);

        match &result[0] {
            acp::McpServer::Http(s) => {
                assert_eq!(s.name, "linear");
                assert_eq!(s.url, server_url);
                assert_eq!(s.headers.len(), 1);
                assert_eq!(s.headers[0].name, "Authorization");
                assert_eq!(s.headers[0].value, "Bearer test-oauth-access-token");
            }
            other => panic!("Expected Http, got {other:?}"),
        }

        // Cleanup
        match old_codex_home {
            Some(val) => unsafe { std::env::set_var("CODEX_HOME", val) },
            None => unsafe { std::env::remove_var("CODEX_HOME") },
        }
    }

    #[test]
    #[serial]
    fn bearer_token_env_var_takes_precedence_over_stored_oauth() {
        use codex_rmcp_client::OAuthCredentialsStoreMode;
        use codex_rmcp_client::StoredOAuthTokens;
        use codex_rmcp_client::WrappedOAuthTokenResponse;
        use codex_rmcp_client::save_oauth_tokens;
        use oauth2::AccessToken;
        use oauth2::EmptyExtraTokenFields;
        use oauth2::basic::BasicTokenType;
        use rmcp::transport::auth::OAuthTokenResponse;

        let tmp_dir = tempfile::tempdir().unwrap();
        let tmp_path = tmp_dir.path().to_str().unwrap().to_string();

        let old_codex_home = std::env::var("CODEX_HOME").ok();
        unsafe { std::env::set_var("CODEX_HOME", &tmp_path) };

        let server_name = "my-server";
        let server_url = "https://mcp.example.com";

        // Store OAuth tokens
        let token_response = OAuthTokenResponse::new(
            AccessToken::new("oauth-token-should-be-ignored".to_string()),
            BasicTokenType::Bearer,
            EmptyExtraTokenFields {},
        );
        let stored = StoredOAuthTokens {
            server_name: server_name.to_string(),
            url: server_url.to_string(),
            client_id: "test-client-id".to_string(),
            token_response: WrappedOAuthTokenResponse(token_response),
            expires_at: None,
        };
        save_oauth_tokens(server_name, &stored, OAuthCredentialsStoreMode::File).unwrap();

        // Also set a bearer token env var
        unsafe { std::env::set_var("TEST_PRECEDENCE_TOKEN", "env-var-token-wins") };

        let mut servers = HashMap::new();
        servers.insert(
            server_name.to_string(),
            http_config(
                server_url,
                Some("TEST_PRECEDENCE_TOKEN".to_string()),
                None,
                None,
            ),
        );

        let result = to_sacp_mcp_servers(&servers);
        assert_eq!(result.len(), 1);

        match &result[0] {
            acp::McpServer::Http(s) => {
                // Should have exactly one Authorization header from env var, not OAuth
                let auth_headers: Vec<_> = s
                    .headers
                    .iter()
                    .filter(|h| h.name == "Authorization")
                    .collect();
                assert_eq!(auth_headers.len(), 1);
                assert_eq!(auth_headers[0].value, "Bearer env-var-token-wins");
            }
            other => panic!("Expected Http, got {other:?}"),
        }

        // Cleanup
        unsafe { std::env::remove_var("TEST_PRECEDENCE_TOKEN") };
        match old_codex_home {
            Some(val) => unsafe { std::env::set_var("CODEX_HOME", val) },
            None => unsafe { std::env::remove_var("CODEX_HOME") },
        }
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
