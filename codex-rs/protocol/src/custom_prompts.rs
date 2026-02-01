use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::path::PathBuf;
use ts_rs::TS;

/// Base namespace for custom prompt slash commands (without trailing colon).
/// Example usage forms constructed in code:
/// - Command token after '/': `"{PROMPTS_CMD_PREFIX}:name"`
/// - Full slash prefix: `"/{PROMPTS_CMD_PREFIX}:"`
pub const PROMPTS_CMD_PREFIX: &str = "prompts";

/// The kind of a custom prompt: either a static markdown template or an
/// executable script whose stdout becomes the prompt content.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CustomPromptKind {
    /// A markdown template prompt.
    Markdown,
    /// An executable script. `interpreter` is the command used to run it
    /// (e.g. `"bash"`, `"python3"`, `"node"`).
    Script { interpreter: String },
}

impl Default for CustomPromptKind {
    fn default() -> Self {
        Self::Markdown
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema, TS)]
pub struct CustomPrompt {
    pub name: String,
    pub path: PathBuf,
    pub content: String,
    pub description: Option<String>,
    pub argument_hint: Option<String>,
    #[serde(default)]
    pub kind: CustomPromptKind,
}
