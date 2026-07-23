mod auth_status;
mod find_codex_home;
mod logging_client_handler;
mod oauth;
mod perform_oauth_login;
mod program_resolver;
mod rmcp_client;
mod utils;

pub use auth_status::determine_streamable_http_auth_status;
pub use auth_status::supports_oauth_login;
pub use oauth::OAuthCredentialsStoreMode;
pub use oauth::StoredOAuthTokens;
pub use oauth::WrappedOAuthTokenResponse;
pub use oauth::delete_oauth_tokens;
pub use oauth::load_oauth_tokens;
pub use oauth::save_oauth_tokens;
pub use perform_oauth_login::OAuthLoginHandle;
pub use perform_oauth_login::perform_oauth_login;
pub use perform_oauth_login::start_oauth_login;
pub use rmcp::model::ElicitationAction;
pub use rmcp_client::Elicitation;
pub use rmcp_client::ElicitationResponse;
pub use rmcp_client::RmcpClient;
pub use rmcp_client::SendElicitation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpAuthStatus {
    Unsupported,
    NotLoggedIn,
    BearerToken,
    OAuth,
}

impl std::fmt::Display for McpAuthStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Unsupported => "Unsupported",
            Self::NotLoggedIn => "Not logged in",
            Self::BearerToken => "Bearer token",
            Self::OAuth => "OAuth",
        })
    }
}
