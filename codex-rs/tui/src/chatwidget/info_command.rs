pub(super) const INFO_USAGE_HINT: &str = "Usage: /info <your question about nori>";

const INFO_PROMPT_TEMPLATE: &str = include_str!("../../prompt_for_info_command.md");

const DOCS_README: &str = include_str!("../../../../README.md");
const DOCS_OVERVIEW: &str = include_str!("../../../../docs.md");
const DOCS_CLI: &str = include_str!("../../../cli/docs.md");

/// Composes a documentation-augmented prompt for the `/info` slash command.
///
/// Given a user question, this returns a prompt that includes relevant
/// nori documentation context so the model can answer end-user questions.
/// Returns `None` if the question is empty or whitespace-only.
pub(super) fn compose_info_prompt(question: &str) -> Option<String> {
    let trimmed = question.trim();
    if trimmed.is_empty() {
        return None;
    }

    let docs = format!("{DOCS_README}\n\n---\n\n{DOCS_OVERVIEW}\n\n---\n\n{DOCS_CLI}");
    let prompt = INFO_PROMPT_TEMPLATE
        .replace("$DOCS", &docs)
        .replace("$QUESTION", trimmed);
    Some(prompt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_info_prompt_includes_question() {
        let prompt = compose_info_prompt("how do I configure agents?")
            .expect("should return Some for non-empty question");
        assert!(
            prompt.contains("how do I configure agents?"),
            "prompt should contain the user's question"
        );
    }

    #[test]
    fn compose_info_prompt_replaces_template_placeholders() {
        let prompt = compose_info_prompt("what is nori?")
            .expect("should return Some for non-empty question");
        // Template placeholders should be replaced with actual content
        assert!(
            !prompt.contains("$DOCS"),
            "prompt should have $DOCS placeholder replaced"
        );
        assert!(
            !prompt.contains("$QUESTION"),
            "prompt should have $QUESTION placeholder replaced"
        );
    }

    #[test]
    fn compose_info_prompt_returns_none_for_empty() {
        assert!(compose_info_prompt("").is_none());
        assert!(compose_info_prompt("   ").is_none());
        assert!(compose_info_prompt("\n\t").is_none());
    }
}
