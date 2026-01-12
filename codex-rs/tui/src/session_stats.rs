//! Session statistics tracking and display.
//!
//! Tracks message counts, tool calls, skills used, and subagents invoked
//! during a conversation session. Displays as a bordered table at session end.

use crate::history_cell::HistoryCell;
use crate::history_cell::with_border;
use ratatui::prelude::*;
use ratatui::style::Stylize;
use std::collections::HashMap;

/// Tracks statistics for a conversation session.
#[derive(Debug, Default, Clone)]
pub struct SessionStats {
    /// Number of user messages sent
    pub user_messages: u32,
    /// Number of assistant messages received
    pub assistant_messages: u32,
    /// Tool calls by category (e.g., "Bash", "Read", "Edit")
    pub tool_calls: HashMap<String, u32>,
    /// Skills that were invoked during the session
    pub skills_used: Vec<String>,
    /// Subagents that were invoked during the session
    pub subagents_used: Vec<String>,
}

impl SessionStats {
    /// Create a new empty session stats tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a user message.
    pub fn record_user_message(&mut self) {
        self.user_messages += 1;
    }

    /// Record an assistant message.
    pub fn record_assistant_message(&mut self) {
        self.assistant_messages += 1;
    }

    /// Record a tool call by category.
    pub fn record_tool_call(&mut self, category: &str) {
        *self.tool_calls.entry(category.to_string()).or_insert(0) += 1;
    }

    /// Record a skill being used. Only adds if not already recorded.
    pub fn record_skill(&mut self, skill_name: &str) {
        if !self.skills_used.contains(&skill_name.to_string()) {
            self.skills_used.push(skill_name.to_string());
        }
    }

    /// Record a subagent being used. Only adds if not already recorded.
    pub fn record_subagent(&mut self, subagent_type: &str) {
        if !self.subagents_used.contains(&subagent_type.to_string()) {
            self.subagents_used.push(subagent_type.to_string());
        }
    }

    /// Check if any statistics have been recorded.
    pub fn has_activity(&self) -> bool {
        self.user_messages > 0
            || self.assistant_messages > 0
            || !self.tool_calls.is_empty()
            || !self.skills_used.is_empty()
            || !self.subagents_used.is_empty()
    }
}

/// A history cell that displays session statistics in a bordered table.
#[derive(Debug)]
pub struct SessionStatisticsCell {
    stats: SessionStats,
}

impl SessionStatisticsCell {
    /// Create a new session statistics display cell.
    pub fn new(stats: SessionStats) -> Self {
        Self { stats }
    }
}

impl HistoryCell for SessionStatisticsCell {
    fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
        let mut content_lines: Vec<Line<'static>> = Vec::new();

        // Title
        content_lines.push(Line::from(vec![
            Span::from("Nori Session Statistics").bold(),
        ]));
        content_lines.push(Line::from(""));

        // Messages section
        let messages_line = Line::from(vec![
            Span::from("Messages").bold(),
            Span::from("          "),
            Span::from(format!("User: {}", self.stats.user_messages)),
            Span::from("    "),
            Span::from(format!("Assistant: {}", self.stats.assistant_messages)),
        ]);
        content_lines.push(messages_line);

        // Tool Calls section
        let tool_calls_text = if self.stats.tool_calls.is_empty() {
            "(none)".to_string()
        } else {
            let mut sorted: Vec<_> = self.stats.tool_calls.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(b.0));
            sorted
                .iter()
                .map(|(name, count)| format!("{name}: {count}"))
                .collect::<Vec<_>>()
                .join("  ")
        };
        let tool_calls_line = Line::from(vec![
            Span::from("Tool Calls").bold(),
            Span::from("        "),
            Span::from(tool_calls_text),
        ]);
        content_lines.push(tool_calls_line);
        content_lines.push(Line::from(""));

        // Skills Used section
        content_lines.push(Line::from(vec![Span::from("Skills Used").bold()]));
        if self.stats.skills_used.is_empty() {
            content_lines.push(Line::from("  (none)"));
        } else {
            for skill in &self.stats.skills_used {
                content_lines.push(Line::from(format!("  {skill}")));
            }
        }
        content_lines.push(Line::from(""));

        // Subagents Used section
        content_lines.push(Line::from(vec![Span::from("Subagents Used").bold()]));
        if self.stats.subagents_used.is_empty() {
            content_lines.push(Line::from("  (none)"));
        } else {
            for subagent in &self.stats.subagents_used {
                content_lines.push(Line::from(format!("  {subagent}")));
            }
        }

        with_border(content_lines)
    }
}

/// Extract skill name from a Skill tool call's raw_input JSON.
///
/// The Skill tool is invoked with `{"skill": "skill-name"}`.
pub fn extract_skill_from_raw_input(raw_input: Option<&serde_json::Value>) -> Option<String> {
    raw_input
        .and_then(|v| v.get("skill"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Extract subagent type from a Task tool call's raw_input JSON.
///
/// The Task tool is invoked with `{"subagent_type": "agent-type", ...}`.
pub fn extract_subagent_from_raw_input(raw_input: Option<&serde_json::Value>) -> Option<String> {
    raw_input
        .and_then(|v| v.get("subagent_type"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Extract skill name from a Read tool call's file path.
///
/// Matches any path ending in `{skill-name}/SKILL.md`.
/// Returns the skill name (directory name) if the path matches, None otherwise.
pub fn extract_skill_from_read_path(file_path: Option<&str>) -> Option<String> {
    use regex_lite::Regex;

    let path = file_path?;

    // Match any path ending in {skill-name}/SKILL.md
    let re = Regex::new(r"([^/]+)/SKILL\.md$").ok()?;

    re.captures(path)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
}

/// Extract all skill names from text content (e.g., Task tool output).
///
/// Scans text for patterns like `{skill-name}/SKILL.md` and returns
/// all unique skill names found. This is used to detect skills used
/// by subagents whose tool calls are not directly visible.
pub fn extract_skills_from_text(text: &str) -> Vec<String> {
    use regex_lite::Regex;

    let mut skills = Vec::new();

    // Match patterns like "skill-name/SKILL.md" anywhere in text
    if let Ok(re) = Regex::new(r"([a-zA-Z0-9_-]+)/SKILL\.md") {
        for cap in re.captures_iter(text) {
            if let Some(skill_name) = cap.get(1) {
                let name = skill_name.as_str().to_string();
                if !skills.contains(&name) {
                    skills.push(name);
                }
            }
        }
    }

    skills
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // =========================================================================
    // RED PHASE: Tests for SessionStats struct
    // These tests define expected behavior before implementation
    // =========================================================================

    #[test]
    fn new_session_stats_is_empty() {
        let stats = SessionStats::new();
        assert_eq!(stats.user_messages, 0);
        assert_eq!(stats.assistant_messages, 0);
        assert!(stats.tool_calls.is_empty());
        assert!(stats.skills_used.is_empty());
        assert!(stats.subagents_used.is_empty());
    }

    #[test]
    fn record_user_message_increments_count() {
        let mut stats = SessionStats::new();
        stats.record_user_message();
        assert_eq!(stats.user_messages, 1);
        stats.record_user_message();
        assert_eq!(stats.user_messages, 2);
    }

    #[test]
    fn record_assistant_message_increments_count() {
        let mut stats = SessionStats::new();
        stats.record_assistant_message();
        assert_eq!(stats.assistant_messages, 1);
        stats.record_assistant_message();
        assert_eq!(stats.assistant_messages, 2);
    }

    #[test]
    fn record_tool_call_tracks_by_category() {
        let mut stats = SessionStats::new();
        stats.record_tool_call("Bash");
        stats.record_tool_call("Read");
        stats.record_tool_call("Bash");

        assert_eq!(stats.tool_calls.get("Bash"), Some(&2));
        assert_eq!(stats.tool_calls.get("Read"), Some(&1));
        assert_eq!(stats.tool_calls.get("Edit"), None);
    }

    #[test]
    fn record_skill_adds_unique_skills() {
        let mut stats = SessionStats::new();
        stats.record_skill("brainstorming");
        stats.record_skill("tdd");
        stats.record_skill("brainstorming"); // duplicate

        assert_eq!(stats.skills_used.len(), 2);
        assert!(stats.skills_used.contains(&"brainstorming".to_string()));
        assert!(stats.skills_used.contains(&"tdd".to_string()));
    }

    #[test]
    fn record_subagent_adds_unique_subagents() {
        let mut stats = SessionStats::new();
        stats.record_subagent("nori-codebase-locator");
        stats.record_subagent("nori-knowledge-researcher");
        stats.record_subagent("nori-codebase-locator"); // duplicate

        assert_eq!(stats.subagents_used.len(), 2);
        assert!(
            stats
                .subagents_used
                .contains(&"nori-codebase-locator".to_string())
        );
        assert!(
            stats
                .subagents_used
                .contains(&"nori-knowledge-researcher".to_string())
        );
    }

    #[test]
    fn has_activity_false_when_empty() {
        let stats = SessionStats::new();
        assert!(!stats.has_activity());
    }

    #[test]
    fn has_activity_true_with_user_message() {
        let mut stats = SessionStats::new();
        stats.record_user_message();
        assert!(stats.has_activity());
    }

    #[test]
    fn has_activity_true_with_tool_call() {
        let mut stats = SessionStats::new();
        stats.record_tool_call("Bash");
        assert!(stats.has_activity());
    }

    #[test]
    fn has_activity_true_with_skill() {
        let mut stats = SessionStats::new();
        stats.record_skill("tdd");
        assert!(stats.has_activity());
    }

    #[test]
    fn has_activity_true_with_subagent() {
        let mut stats = SessionStats::new();
        stats.record_subagent("nori-codebase-locator");
        assert!(stats.has_activity());
    }

    // =========================================================================
    // RED PHASE: Tests for skill/subagent detection from raw_input
    // =========================================================================

    #[test]
    fn extract_skill_from_valid_raw_input() {
        let raw_input = json!({"skill": "brainstorming"});
        let result = extract_skill_from_raw_input(Some(&raw_input));
        assert_eq!(result, Some("brainstorming".to_string()));
    }

    #[test]
    fn extract_skill_from_none_returns_none() {
        let result = extract_skill_from_raw_input(None);
        assert_eq!(result, None);
    }

    #[test]
    fn extract_skill_from_missing_field_returns_none() {
        let raw_input = json!({"other": "value"});
        let result = extract_skill_from_raw_input(Some(&raw_input));
        assert_eq!(result, None);
    }

    #[test]
    fn extract_subagent_from_valid_raw_input() {
        let raw_input = json!({
            "description": "Find files",
            "prompt": "Search for test files",
            "subagent_type": "nori-codebase-locator"
        });
        let result = extract_subagent_from_raw_input(Some(&raw_input));
        assert_eq!(result, Some("nori-codebase-locator".to_string()));
    }

    #[test]
    fn extract_subagent_from_none_returns_none() {
        let result = extract_subagent_from_raw_input(None);
        assert_eq!(result, None);
    }

    #[test]
    fn extract_subagent_from_missing_field_returns_none() {
        let raw_input = json!({"description": "test", "prompt": "test"});
        let result = extract_subagent_from_raw_input(Some(&raw_input));
        assert_eq!(result, None);
    }

    // =========================================================================
    // Tests for skill extraction from Read tool file paths
    // =========================================================================

    #[test]
    fn extract_skill_from_read_path_with_absolute_path() {
        let result =
            extract_skill_from_read_path(Some("/home/user/.claude/skills/brainstorming/SKILL.md"));
        assert_eq!(result, Some("brainstorming".to_string()));
    }

    #[test]
    fn extract_skill_from_read_path_with_tilde_path() {
        let result =
            extract_skill_from_read_path(Some("~/.claude/skills/test-driven-development/SKILL.md"));
        assert_eq!(result, Some("test-driven-development".to_string()));
    }

    #[test]
    fn extract_skill_from_read_path_with_any_skill_md() {
        // Should match any path ending in {name}/SKILL.md
        let result = extract_skill_from_read_path(Some("/some/random/path/my-skill/SKILL.md"));
        assert_eq!(result, Some("my-skill".to_string()));
    }

    #[test]
    fn extract_skill_from_read_path_with_non_skill_path() {
        let result = extract_skill_from_read_path(Some("/home/user/code/project/src/main.rs"));
        assert_eq!(result, None);
    }

    #[test]
    fn extract_skill_from_read_path_with_none() {
        let result = extract_skill_from_read_path(None);
        assert_eq!(result, None);
    }

    #[test]
    fn extract_skill_from_read_path_with_partial_skill_path() {
        // Not a SKILL.md file
        let result =
            extract_skill_from_read_path(Some("/home/user/.claude/skills/brainstorming/README.md"));
        assert_eq!(result, None);
    }

    // =========================================================================
    // Tests for extracting skills from text content (Task tool output)
    // =========================================================================

    #[test]
    fn extract_skills_from_text_finds_single_skill() {
        let text = "Reading /home/user/.claude/skills/brainstorming/SKILL.md";
        let result = extract_skills_from_text(text);
        assert_eq!(result, vec!["brainstorming".to_string()]);
    }

    #[test]
    fn extract_skills_from_text_finds_multiple_skills() {
        let text = r#"
            Read file: /home/user/.claude/skills/using-skills/SKILL.md
            Then read: ~/.claude/skills/test-driven-development/SKILL.md
            And finally: /path/to/brainstorming/SKILL.md
        "#;
        let result = extract_skills_from_text(text);
        assert_eq!(result.len(), 3);
        assert!(result.contains(&"using-skills".to_string()));
        assert!(result.contains(&"test-driven-development".to_string()));
        assert!(result.contains(&"brainstorming".to_string()));
    }

    #[test]
    fn extract_skills_from_text_deduplicates() {
        let text = r#"
            Read: /home/user/.claude/skills/tdd/SKILL.md
            Read again: ~/.claude/skills/tdd/SKILL.md
        "#;
        let result = extract_skills_from_text(text);
        assert_eq!(result, vec!["tdd".to_string()]);
    }

    #[test]
    fn extract_skills_from_text_returns_empty_for_no_skills() {
        let text = "Just some regular text with no skill paths";
        let result = extract_skills_from_text(text);
        assert!(result.is_empty());
    }

    #[test]
    fn extract_skills_from_text_ignores_non_skill_md_files() {
        let text = r#"
            Read: /home/user/.claude/skills/brainstorming/README.md
            Read: /home/user/code/project/src/main.rs
        "#;
        let result = extract_skills_from_text(text);
        assert!(result.is_empty());
    }

    #[test]
    fn extract_skills_from_text_handles_underscores_and_dashes() {
        let text = "Read: /path/to/my_skill-name/SKILL.md";
        let result = extract_skills_from_text(text);
        assert_eq!(result, vec!["my_skill-name".to_string()]);
    }

    // =========================================================================
    // RED PHASE: Tests for SessionStatisticsCell display
    // These tests will fail until display_lines is implemented
    // =========================================================================

    #[test]
    fn display_lines_shows_title() {
        let stats = SessionStats::new();
        let cell = SessionStatisticsCell::new(stats);
        let lines = cell.display_lines(60);

        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();

        assert!(
            text.contains("Nori Session Statistics"),
            "Expected title in output"
        );
    }

    #[test]
    fn display_lines_shows_message_counts() {
        let mut stats = SessionStats::new();
        stats.user_messages = 5;
        stats.assistant_messages = 7;

        let cell = SessionStatisticsCell::new(stats);
        let lines = cell.display_lines(60);

        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();

        assert!(text.contains("Messages"), "Expected Messages label");
        assert!(text.contains("User: 5"), "Expected user count");
        assert!(text.contains("Assistant: 7"), "Expected assistant count");
    }

    #[test]
    fn display_lines_shows_tool_calls() {
        let mut stats = SessionStats::new();
        stats.tool_calls.insert("Bash".to_string(), 3);
        stats.tool_calls.insert("Read".to_string(), 2);

        let cell = SessionStatisticsCell::new(stats);
        let lines = cell.display_lines(60);

        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();

        assert!(text.contains("Tool Calls"), "Expected Tool Calls label");
        assert!(text.contains("Bash: 3"), "Expected Bash count");
        assert!(text.contains("Read: 2"), "Expected Read count");
    }

    #[test]
    fn display_lines_shows_no_tool_calls_when_empty() {
        let stats = SessionStats::new();
        let cell = SessionStatisticsCell::new(stats);
        let lines = cell.display_lines(60);

        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();

        // When no tool calls, should show "(none)" or similar
        assert!(
            text.contains("Tool Calls") && (text.contains("(none)") || text.contains("none")),
            "Expected indication of no tool calls"
        );
    }

    #[test]
    fn display_lines_shows_skills_used() {
        let mut stats = SessionStats::new();
        stats.skills_used.push("brainstorming".to_string());
        stats.skills_used.push("tdd".to_string());

        let cell = SessionStatisticsCell::new(stats);
        let lines = cell.display_lines(60);

        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();

        assert!(text.contains("Skills Used"), "Expected Skills Used section");
        assert!(
            text.contains("brainstorming"),
            "Expected brainstorming skill"
        );
        assert!(text.contains("tdd"), "Expected tdd skill");
    }

    #[test]
    fn display_lines_shows_no_skills_when_empty() {
        let stats = SessionStats::new();
        let cell = SessionStatisticsCell::new(stats);
        let lines = cell.display_lines(60);

        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();

        assert!(text.contains("Skills Used"), "Expected Skills Used section");
        assert!(
            text.contains("(none)") || text.to_lowercase().contains("none"),
            "Expected indication of no skills"
        );
    }

    #[test]
    fn display_lines_shows_subagents_used() {
        let mut stats = SessionStats::new();
        stats
            .subagents_used
            .push("nori-codebase-locator".to_string());
        stats
            .subagents_used
            .push("nori-knowledge-researcher".to_string());

        let cell = SessionStatisticsCell::new(stats);
        let lines = cell.display_lines(60);

        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();

        assert!(
            text.contains("Subagents Used"),
            "Expected Subagents Used section"
        );
        assert!(
            text.contains("nori-codebase-locator"),
            "Expected locator subagent"
        );
        assert!(
            text.contains("nori-knowledge-researcher"),
            "Expected researcher subagent"
        );
    }

    #[test]
    fn display_lines_shows_no_subagents_when_empty() {
        let stats = SessionStats::new();
        let cell = SessionStatisticsCell::new(stats);
        let lines = cell.display_lines(60);

        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();

        assert!(
            text.contains("Subagents Used"),
            "Expected Subagents Used section"
        );
        assert!(
            text.contains("(none)") || text.to_lowercase().contains("none"),
            "Expected indication of no subagents"
        );
    }

    #[test]
    fn display_lines_has_border() {
        let stats = SessionStats::new();
        let cell = SessionStatisticsCell::new(stats);
        let lines = cell.display_lines(60);

        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();

        // Should have box-drawing characters for border
        assert!(
            text.contains("╭") || text.contains("┌"),
            "Expected top-left corner"
        );
        assert!(
            text.contains("╯") || text.contains("┘"),
            "Expected bottom-right corner"
        );
    }
}
