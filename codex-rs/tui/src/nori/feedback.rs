//! Nori-specific feedback handling - redirects to GitHub Discussions

pub const NORI_FEEDBACK_URL: &str = "https://github.com/tilework-tech/nori-cli/discussions";

pub fn feedback_message() -> &'static str {
    "To report issues or provide feedback, please visit:\n\
     https://github.com/tilework-tech/nori-cli/discussions"
}
