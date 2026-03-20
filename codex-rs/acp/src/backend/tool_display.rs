use super::*;

/// Truncate a string for logging purposes
pub(crate) fn truncate_for_log(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let safe = codex_utils_string::take_bytes_at_char_boundary(s, max_len);
        format!("{safe}...")
    }
}

/// Check if a tool call title contains useful display information.
///
/// Some ACP providers include the path/command directly in the title
/// (e.g., "Read /home/user/file.rs" or "`git status`") rather than in raw_input.
/// This function detects such cases so we don't skip them.
pub(crate) fn title_contains_useful_info(title: &str) -> bool {
    // Check for absolute paths (Unix or Windows style)
    if title.contains(" /") || title.contains(" C:\\") || title.contains(" ~") {
        return true;
    }
    // Check for backtick-quoted commands (e.g., "`git status`")
    if title.contains('`') {
        return true;
    }
    // Check for patterns that suggest it's not a generic title
    // Generic titles are typically just the tool name like "Read File", "Terminal", "Search"
    let generic_patterns = [
        "Read File",
        "Read file",
        "Terminal",
        "Search",
        "Grep",
        "Glob",
        "List",
        "Write",
        "Edit",
    ];
    for pattern in &generic_patterns {
        if title == *pattern {
            return false;
        }
    }
    // If the title is longer than typical generic names and contains a space,
    // it likely has useful info
    title.len() > 15 && title.contains(' ')
}

/// Format a tool call command with its input arguments for display.
///
/// Creates a display string like "Read(path/to/file.rs)" or "Terminal(git status)"
pub(crate) fn format_tool_call_command(
    title: &str,
    raw_input: Option<&serde_json::Value>,
) -> String {
    let args = raw_input
        .and_then(|input| extract_display_args(title, input))
        .unwrap_or_default();

    if args.is_empty() {
        title.to_string()
    } else if title.contains(&args) {
        // Don't append args if they're already contained in the title
        title.to_string()
    } else {
        format!("{title}({args})")
    }
}

/// Extract display-friendly arguments from raw_input based on tool type.
pub(crate) fn extract_display_args(title: &str, input: &serde_json::Value) -> Option<String> {
    let title_lower = title.to_lowercase();

    // Try to extract the most relevant argument based on tool type
    // Note: Order matters - more specific matches should come first
    if title_lower.contains("search")
        || title_lower.contains("find")
        || title_lower.contains("grep")
    {
        // For search operations, show the pattern/query
        let pattern = input
            .get("pattern")
            .or_else(|| input.get("query"))
            .or_else(|| input.get("glob"))
            .and_then(|v| v.as_str());
        let path = input.get("path").and_then(|v| v.as_str());

        match (pattern, path) {
            (Some(p), Some(dir)) => Some(format!("{p} in {dir}")),
            (Some(p), None) => Some(p.to_string()),
            (None, Some(dir)) => Some(dir.to_string()),
            (None, None) => None,
        }
    } else if title_lower.contains("terminal")
        || title_lower.contains("shell")
        || title_lower.contains("bash")
        || title_lower.contains("exec")
    {
        // For shell commands, show the command
        input
            .get("command")
            .or_else(|| input.get("cmd"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    } else if title_lower.contains("list") || title_lower.contains("ls") {
        // For list operations, show the path
        input
            .get("path")
            .or_else(|| input.get("directory"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    } else if title_lower.contains("write") || title_lower.contains("edit") {
        // For write operations, show the path
        input
            .get("path")
            .or_else(|| input.get("file_path"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    } else if title_lower.contains("read") || title_lower.contains("file") {
        // For file read operations, show the path
        input
            .get("path")
            .or_else(|| input.get("file_path"))
            .or_else(|| input.get("file"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    } else {
        // Generic fallback: try common argument names
        input
            .get("path")
            .or_else(|| input.get("command"))
            .or_else(|| input.get("query"))
            .or_else(|| input.get("name"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    }
}

/// Extract tool output from ToolCallUpdateFields for display.
///
/// Returns a formatted string containing the tool's output content.
/// Prioritizes rawOutput fields (Codex format) over content field, and strips
/// markdown code blocks from the output.
pub(crate) fn extract_tool_output(fields: &acp::ToolCallUpdateFields) -> String {
    // Try rawOutput first (Codex provides structured output here)
    if let Some(raw_output) = &fields.raw_output {
        // Try to extract stdout (most common for shell commands)
        if let Some(stdout) = raw_output.get("stdout").and_then(|v| v.as_str())
            && !stdout.is_empty()
        {
            return strip_markdown_code_blocks(stdout);
        }

        // Try formatted_output next
        if let Some(formatted) = raw_output.get("formatted_output").and_then(|v| v.as_str())
            && !formatted.is_empty()
        {
            return strip_markdown_code_blocks(formatted);
        }

        // Try aggregated_output as fallback
        if let Some(aggregated) = raw_output.get("aggregated_output").and_then(|v| v.as_str())
            && !aggregated.is_empty()
        {
            return strip_markdown_code_blocks(aggregated);
        }

        // If none of the direct fields worked, try format_raw_output for summaries
        if let Some(output_str) = format_raw_output(raw_output, fields.title.as_deref()) {
            return output_str;
        }
    }

    // Fallback to content field (existing behavior for non-Codex agents)
    let mut output_parts: Vec<String> = Vec::new();
    if let Some(content) = &fields.content {
        for item in content {
            if let acp::ToolCallContent::Content(c) = item
                && let acp::ContentBlock::Text(text) = &c.content
                && !text.text.is_empty()
            {
                // Strip markdown from content field too
                output_parts.push(strip_markdown_code_blocks(&text.text));
            }
        }
    }

    output_parts.join("\n")
}

/// Strip markdown code block formatting from output.
///
/// Codex wraps output in markdown code blocks like:
/// ````text
/// ```sh
/// output here
/// ```
/// ````
///
/// This function removes the wrapper and returns just the content.
pub(crate) fn strip_markdown_code_blocks(text: &str) -> String {
    let text = text.trim();

    // Check for code block pattern: ```language\n...\n```
    if text.starts_with("```") {
        // Find the end of the opening marker (first newline after ```)
        if let Some(start) = text.find('\n') {
            // Find the closing ```
            if let Some(end) = text.rfind("\n```") {
                // Extract content between markers
                return text[start + 1..end].to_string();
            }
        }
    }

    // No markdown wrapper found, return as-is
    text.to_string()
}

/// Format raw_output JSON into a human-readable string based on tool type.
pub(crate) fn format_raw_output(
    raw_output: &serde_json::Value,
    title: Option<&str>,
) -> Option<String> {
    let title_lower = title.map(str::to_lowercase).unwrap_or_default();

    // Try to provide meaningful summaries based on common output patterns
    if let Some(obj) = raw_output.as_object() {
        // Check for line count (common in read operations)
        if let Some(lines) = obj.get("lines").and_then(serde_json::Value::as_u64) {
            return Some(format!("Read {lines} lines"));
        }

        // Check for file count (common in find/search operations)
        if let Some(count) = obj.get("count").and_then(serde_json::Value::as_u64) {
            if title_lower.contains("find") || title_lower.contains("search") {
                return Some(format!("Found {count} files"));
            }
            return Some(format!("{count} matches"));
        }

        // Check for files array
        if let Some(files) = obj.get("files").and_then(|v| v.as_array()) {
            let count = files.len();
            let file_list: Vec<&str> = files.iter().filter_map(|f| f.as_str()).take(5).collect();
            if count > 5 {
                return Some(format!(
                    "Found {} files\n{}...",
                    count,
                    file_list.join("\n")
                ));
            } else if !file_list.is_empty() {
                return Some(format!("Found {} files\n{}", count, file_list.join("\n")));
            }
        }

        // Check for exit_code (common in shell operations)
        if let Some(exit_code) = obj.get("exit_code").and_then(serde_json::Value::as_i64) {
            // Look for stdout/output
            let output = obj
                .get("stdout")
                .or_else(|| obj.get("output"))
                .and_then(|v| v.as_str());
            if let Some(out) = output {
                if exit_code != 0 {
                    return Some(format!("Exit code: {exit_code}\n{out}"));
                }
                return Some(out.to_string());
            }
            if exit_code != 0 {
                return Some(format!("Exit code: {exit_code}"));
            }
        }

        // Check for success boolean
        if let Some(success) = obj.get("success").and_then(serde_json::Value::as_bool)
            && !success
        {
            if let Some(error) = obj.get("error").and_then(|v| v.as_str()) {
                return Some(format!("Failed: {error}"));
            }
            return Some("Operation failed".to_string());
        }
    }

    // For arrays, show count
    if let Some(arr) = raw_output.as_array()
        && !arr.is_empty()
    {
        return Some(format!("{} items", arr.len()));
    }

    // For strings, return directly
    if let Some(s) = raw_output.as_str()
        && !s.is_empty()
    {
        return Some(s.to_string());
    }

    None
}

/// Classify a tool call into ParsedCommand variants based on ACP ToolKind.
///
/// This enables the TUI to render tool calls appropriately:
/// - `Read`, `ListFiles`, `Search` → "Exploring" mode with compact, grouped display
/// - `Unknown` → "Command" mode with full command text display
///
/// # ACP ToolKind mappings:
/// - `Read` → `ParsedCommand::Read` (exploring)
/// - `Search` → `ParsedCommand::Search` (exploring)
/// - `Edit`, `Delete`, `Move`, `Execute`, `Fetch` → `ParsedCommand::Unknown` (command)
/// - `Think`, `Other` → `ParsedCommand::Unknown` (command)
pub(crate) fn classify_tool_to_parsed_command(
    title: &str,
    kind: Option<&acp::ToolKind>,
    raw_input: Option<&serde_json::Value>,
) -> Vec<ParsedCommand> {
    match kind {
        // Read operations → Exploring mode
        Some(acp::ToolKind::Read) => {
            let path = raw_input
                .and_then(|i| {
                    i.get("path")
                        .or_else(|| i.get("file_path"))
                        .or_else(|| i.get("file"))
                })
                .and_then(|v| v.as_str())
                .unwrap_or("file");
            let name = std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.to_string());
            vec![ParsedCommand::Read {
                cmd: title.to_string(),
                name,
                path: std::path::PathBuf::from(path),
            }]
        }

        // Search operations → Exploring mode
        Some(acp::ToolKind::Search) => {
            let query = raw_input
                .and_then(|i| i.get("pattern").or_else(|| i.get("query")))
                .and_then(|v| v.as_str())
                .map(String::from);
            let path = raw_input
                .and_then(|i| i.get("path").or_else(|| i.get("directory")))
                .and_then(|v| v.as_str())
                .map(String::from);
            vec![ParsedCommand::Search {
                cmd: title.to_string(),
                query,
                path,
            }]
        }

        // Edit, Delete, Move → Command mode (mutating operations)
        Some(acp::ToolKind::Edit | acp::ToolKind::Delete | acp::ToolKind::Move) => {
            vec![ParsedCommand::Unknown {
                cmd: format_tool_call_command(title, raw_input),
            }]
        }

        // Execute → Command mode (shell/terminal operations)
        Some(acp::ToolKind::Execute) => {
            vec![ParsedCommand::Unknown {
                cmd: format_tool_call_command(title, raw_input),
            }]
        }

        // Fetch → Command mode (external data retrieval)
        Some(acp::ToolKind::Fetch) => {
            vec![ParsedCommand::Unknown {
                cmd: format_tool_call_command(title, raw_input),
            }]
        }

        // Think → Command mode (internal reasoning)
        Some(acp::ToolKind::Think) => {
            vec![ParsedCommand::Unknown {
                cmd: format_tool_call_command(title, raw_input),
            }]
        }

        // Other or unknown → Command mode (fallback)
        Some(acp::ToolKind::Other) | None => {
            // Try to infer from title as fallback
            classify_tool_by_title(title, raw_input)
        }

        // Catch any future ToolKind variants
        #[allow(unreachable_patterns)]
        _ => {
            vec![ParsedCommand::Unknown {
                cmd: format_tool_call_command(title, raw_input),
            }]
        }
    }
}

/// Fallback classification based on tool title when ToolKind is not available.
///
/// Uses heuristics to detect common tool patterns.
pub(crate) fn classify_tool_by_title(
    title: &str,
    raw_input: Option<&serde_json::Value>,
) -> Vec<ParsedCommand> {
    let title_lower = title.to_lowercase();

    // List/Glob operations → Exploring mode
    if title_lower.contains("list")
        || title_lower.contains("glob")
        || title_lower.contains("ls")
        || title_lower == "find"
        || title_lower.contains("find files")
    {
        let path = raw_input
            .and_then(|i| i.get("path").or_else(|| i.get("directory")))
            .and_then(|v| v.as_str())
            .map(String::from);
        return vec![ParsedCommand::ListFiles {
            cmd: title.to_string(),
            path,
        }];
    }

    // Search/Grep operations → Exploring mode
    if title_lower.contains("search") || title_lower.contains("grep") {
        let query = raw_input
            .and_then(|i| i.get("pattern").or_else(|| i.get("query")))
            .and_then(|v| v.as_str())
            .map(String::from);
        let path = raw_input
            .and_then(|i| i.get("path"))
            .and_then(|v| v.as_str())
            .map(String::from);
        return vec![ParsedCommand::Search {
            cmd: title.to_string(),
            query,
            path,
        }];
    }

    // Read operations → Exploring mode
    if title_lower.contains("read") || title_lower == "file" {
        let path = raw_input
            .and_then(|i| {
                i.get("path")
                    .or_else(|| i.get("file_path"))
                    .or_else(|| i.get("file"))
            })
            .and_then(|v| v.as_str())
            .unwrap_or("file");
        let name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());
        return vec![ParsedCommand::Read {
            cmd: title.to_string(),
            name,
            path: std::path::PathBuf::from(path),
        }];
    }

    // Default: Command mode
    vec![ParsedCommand::Unknown {
        cmd: format_tool_call_command(title, raw_input),
    }]
}

/// Extract the actual shell command from a Gemini permission request title.
///
/// Gemini's `run_shell_command` permission request titles follow the pattern:
///   `<command> [current working directory <path>] (<description>)`
///
/// Examples:
///   `echo "hello" [current working directory /home/user/project] (Running echo)`
///     → `echo "hello"`
///   `date [current working directory /home/user]`
///     → `date`
///   `git status`
///     → `git status`
pub(crate) fn extract_command_from_permission_title(title: &str) -> String {
    // Look for " [current working directory " marker
    if let Some(cwd_start) = title.find(" [current working directory ") {
        title[..cwd_start].trim().to_string()
    } else {
        title.to_string()
    }
}

/// Returns true if the title looks like a raw Anthropic tool_use ID
/// (e.g., "toolu_015Xtg1GzAd6aPH6oiirx5us").
pub(crate) fn title_is_raw_id(title: &str) -> bool {
    title.starts_with("toolu_")
        && title.len() > 10
        && title[6..].bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Maps a ToolKind to a human-readable display name for fallback use.
pub(crate) fn kind_to_display_name(kind: acp::ToolKind) -> &'static str {
    match kind {
        acp::ToolKind::Read => "Read",
        acp::ToolKind::Edit => "Edit",
        acp::ToolKind::Delete => "Delete",
        acp::ToolKind::Move => "Move",
        acp::ToolKind::Search => "Search",
        acp::ToolKind::Execute => "Terminal",
        acp::ToolKind::Think => "Think",
        acp::ToolKind::Fetch => "Fetch",
        acp::ToolKind::SwitchMode => "Switch Mode",
        acp::ToolKind::Other => "Tool",
        _ => "Tool",
    }
}
