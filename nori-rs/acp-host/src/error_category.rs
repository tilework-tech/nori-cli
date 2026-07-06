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
    /// Unknown error (fallback)
    Unknown,
}

impl AcpErrorCategory {
    /// Whether this error is transient and worth retrying (e.g. for an
    /// unattended loop that should survive a momentary API blip). Server
    /// errors are momentary and rate/quota limits ease over time (seconds for
    /// rate limits, longer for usage windows); everything else reflects a
    /// persistent problem that a retry cannot fix.
    pub fn is_retryable(&self) -> bool {
        match self {
            AcpErrorCategory::ApiServerError | AcpErrorCategory::QuotaExceeded => true,
            AcpErrorCategory::Authentication
            | AcpErrorCategory::ExecutableNotFound
            | AcpErrorCategory::Initialization
            | AcpErrorCategory::PromptTooLong
            | AcpErrorCategory::Unknown => false,
        }
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
