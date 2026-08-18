use nori_protocol::acp::v1 as acp;

/// Categories of ACP spawn errors for providing actionable user messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpErrorCategory {
    /// Authentication required or failed
    Authentication,
    /// Rate limit or quota exceeded
    QuotaExceeded,
    /// Command/executable not found
    ExecutableNotFound,
    /// General initialization failure
    Initialization,
    /// Prompt exceeds the agent's context window
    PromptTooLong,
    /// API returned a server error (5xx)
    ApiServerError,
    /// The requested session no longer exists on the agent (ACP `-32002`)
    SessionNotFound,
    /// The agent's backing service is unreachable (ACP `-32010`)
    AgentUnreachable,
    /// The session exists but cannot be reattached (ACP `-32012`)
    SessionNotResumable,
    /// No session is active on the agent connection (ACP `-32014`)
    NoActiveSession,
    /// A session is already active on the agent connection (ACP `-32015`)
    SessionAlreadyActive,
    /// Unknown error (fallback)
    Unknown,
}

impl AcpErrorCategory {
    /// Whether this error is transient and worth retrying (e.g. for an
    /// unattended loop that should survive a momentary API blip). Server
    /// errors are momentary, rate/quota limits ease over time (seconds for
    /// rate limits, longer for usage windows), and an unreachable backing
    /// service is a network condition; everything else reflects a persistent
    /// problem that a retry cannot fix.
    pub fn is_retryable(&self) -> bool {
        match self {
            AcpErrorCategory::ApiServerError
            | AcpErrorCategory::QuotaExceeded
            | AcpErrorCategory::AgentUnreachable => true,
            AcpErrorCategory::Authentication
            | AcpErrorCategory::ExecutableNotFound
            | AcpErrorCategory::Initialization
            | AcpErrorCategory::PromptTooLong
            | AcpErrorCategory::SessionNotFound
            | AcpErrorCategory::SessionNotResumable
            | AcpErrorCategory::NoActiveSession
            | AcpErrorCategory::SessionAlreadyActive
            | AcpErrorCategory::Unknown => false,
        }
    }
}

/// A categorized ACP error, carrying the agent-supplied message and
/// `error.data.detail` (e.g. a login hint) when the chain includes a structured
/// ACP error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpErrorDetails {
    pub category: AcpErrorCategory,
    pub message: Option<String>,
    pub detail: Option<String>,
}

/// Categorize an error, preferring the structured ACP error (JSON-RPC `code`
/// + `data`) when one is present in the anyhow chain over string heuristics.
///
/// The code table mirrors the agent side (`nori-handroll acp`): `-32000`
/// auth_required, `-32002` resource/session not found, `-32010` backing
/// service unreachable, `-32012` session not resumable, `-32014` no active
/// session, `-32015` a session is already active.
pub fn categorize_acp_error_chain(error: &anyhow::Error) -> AcpErrorDetails {
    for cause in error.chain() {
        let Some(acp_error) = cause.downcast_ref::<acp::Error>() else {
            continue;
        };
        let detail = acp_error.data.as_ref().and_then(|data| {
            data.get("detail")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        });
        let message = Some(acp_error.message.clone());
        let category = match i32::from(acp_error.code) {
            -32000 => AcpErrorCategory::Authentication,
            -32002 => AcpErrorCategory::SessionNotFound,
            -32010 => AcpErrorCategory::AgentUnreachable,
            -32012 => AcpErrorCategory::SessionNotResumable,
            -32014 => AcpErrorCategory::NoActiveSession,
            -32015 => AcpErrorCategory::SessionAlreadyActive,
            _ => categorize_acp_error(&format!("{acp_error:?}")),
        };
        return AcpErrorDetails {
            category,
            message,
            detail,
        };
    }
    AcpErrorDetails {
        category: categorize_acp_error(&format!("{error:?}")),
        message: None,
        detail: None,
    }
}

/// Categorize an ACP error based on error string patterns.
///
/// This function analyzes error messages and categorizes them to enable
/// providing actionable instructions to users.
pub fn categorize_acp_error(error: &str) -> AcpErrorCategory {
    let error_lower = error.to_lowercase();

    if error_lower.contains("auth")
        || error_lower.contains("-32000") // JSON-RPC auth error code
        || error_lower.contains("api key")
        || error_lower.contains("unauthorized")
        || error_lower.contains("not logged in")
    {
        AcpErrorCategory::Authentication
    } else if error_lower.contains("quota")
        || error_lower.contains("rate limit")
        || error_lower.contains("rate_limit") // e.g. Anthropic `rate_limit_error`
        || error_lower.contains("too many requests")
        || error_lower.contains("429")
        || error_lower.contains("out of extra usage")
        || error_lower.contains("usage limit")
        || error_lower.contains("exceeded your usage")
    {
        AcpErrorCategory::QuotaExceeded
    } else if error_lower.contains("command not found")
        || (error_lower.contains("no such file") && error_lower.contains("directory"))
        || error_lower.contains("os error 2") // ENOENT on Unix
        || error_lower.contains("cannot find the path")
    // Windows
    {
        AcpErrorCategory::ExecutableNotFound
    } else if error_lower.contains("initialization")
        || error_lower.contains("handshake")
        || error_lower.contains("protocol")
    {
        AcpErrorCategory::Initialization
    } else if error_lower.contains("prompt is too long") {
        AcpErrorCategory::PromptTooLong
    } else if error_lower.contains("500")
        || error_lower.contains("502")
        || error_lower.contains("503")
        || error_lower.contains("504")
        || error_lower.contains("529") // Anthropic `overloaded_error`
        || error_lower.contains("server error")
        || error_lower.contains("api_error")
        || error_lower.contains("overloaded")
    {
        AcpErrorCategory::ApiServerError
    } else {
        AcpErrorCategory::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// Wrap a structured ACP error the way connection methods surface it: as
    /// the source of an anyhow chain with a human context message on top.
    fn chain_with(acp_error: acp::Error) -> anyhow::Error {
        anyhow::Error::new(acp_error).context("Failed to resume ACP session")
    }

    #[test]
    fn auth_required_categorizes_as_authentication_with_login_detail() {
        let error = chain_with(
            acp::Error::auth_required()
                .data(serde_json::json!({ "detail": "run: nori-handroll login" })),
        );

        let details = categorize_acp_error_chain(&error);

        assert_eq!(
            (
                details.category,
                details.message.as_deref(),
                details.detail.as_deref()
            ),
            (
                AcpErrorCategory::Authentication,
                Some("Authentication required"),
                Some("run: nori-handroll login")
            )
        );
    }

    #[test]
    fn session_lifecycle_codes_map_to_dedicated_categories() {
        let cases = [
            (-32002, AcpErrorCategory::SessionNotFound),
            (-32010, AcpErrorCategory::AgentUnreachable),
            (-32012, AcpErrorCategory::SessionNotResumable),
            (-32014, AcpErrorCategory::NoActiveSession),
            (-32015, AcpErrorCategory::SessionAlreadyActive),
        ];
        for (code, expected) in cases {
            let error = chain_with(acp::Error::new(code, "structured error"));
            assert_eq!(
                categorize_acp_error_chain(&error).category,
                expected,
                "code {code}"
            );
        }
    }

    #[test]
    fn structured_code_takes_precedence_over_message_heuristics() {
        // The message alone would substring-match the auth heuristics; the
        // structured code must win.
        let error = chain_with(acp::Error::new(-32002, "unauthorized session"));

        assert_eq!(
            categorize_acp_error_chain(&error).category,
            AcpErrorCategory::SessionNotFound
        );
    }

    #[test]
    fn falls_back_to_string_heuristics_without_structured_error() {
        let error = anyhow::anyhow!("429 too many requests");

        let details = categorize_acp_error_chain(&error);

        assert_eq!(
            (details.category, details.message, details.detail),
            (AcpErrorCategory::QuotaExceeded, None, None)
        );
    }

    #[test]
    fn broker_unreachable_is_retryable_but_session_states_are_not() {
        assert!(AcpErrorCategory::AgentUnreachable.is_retryable());
        assert!(!AcpErrorCategory::SessionNotFound.is_retryable());
        assert!(!AcpErrorCategory::SessionNotResumable.is_retryable());
        assert!(!AcpErrorCategory::SessionAlreadyActive.is_retryable());
        assert!(!AcpErrorCategory::NoActiveSession.is_retryable());
    }
}
