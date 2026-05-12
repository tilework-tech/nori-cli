use tracing::debug;

pub fn create_patch_with_context(
    path: &std::path::Path,
    cwd: &std::path::Path,
    old_text: &str,
    new_text: &str,
) -> String {
    let full_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };

    let line_offset = if let Ok(file_content) = std::fs::read_to_string(&full_path) {
        file_content
            .find(old_text)
            .or_else(|| file_content.find(new_text))
            .map(|offset| file_content[..offset].lines().count() + 1)
    } else {
        None
    };

    let patch = diffy::create_patch(old_text, new_text).to_string();
    if let Some(offset) = line_offset
        && offset > 1
    {
        return adjust_patch_line_numbers(&patch, offset);
    }
    patch
}

fn adjust_patch_line_numbers(patch: &str, line_offset: usize) -> String {
    let Ok(re) = regex::Regex::new(r"^@@ -(\d+)(,?\d*) \+(\d+)(,?\d*) @@") else {
        return patch.to_string();
    };
    let mut result = String::new();
    for line in patch.lines() {
        if let Some(caps) = re.captures(line) {
            let old_start: usize = caps[1].parse().unwrap_or(1);
            let new_start: usize = caps[3].parse().unwrap_or(1);
            let old_rest = &caps[2];
            let new_rest = &caps[4];

            let adjusted_old_start = old_start + line_offset - 1;
            let adjusted_new_start = new_start + line_offset - 1;

            result.push_str(&format!(
                "@@ -{adjusted_old_start}{old_rest} +{adjusted_new_start}{new_rest} @@\n",
            ));
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }
    result
}

pub(crate) fn try_parse_error_message(text: &str) -> String {
    debug!("Parsing server error response: {}", text);
    let json = serde_json::from_str::<serde_json::Value>(text).unwrap_or_default();
    if let Some(error) = json.get("error")
        && let Some(message) = error.get("message")
        && let Some(message_str) = message.as_str()
    {
        return message_str.to_string();
    }
    if text.is_empty() {
        return "Unknown error".to_string();
    }
    text.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn create_patch_with_context_uses_new_text_when_file_is_already_updated() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let file_path = temp_dir.path().join("story.txt");
        let file_content = (1..=100)
            .map(|line| {
                if line == 50 {
                    "line 50 updated\n".to_string()
                } else {
                    format!("line {line}\n")
                }
            })
            .collect::<String>();
        std::fs::write(&file_path, file_content).expect("write updated file");

        let patch = create_patch_with_context(
            std::path::Path::new("story.txt"),
            temp_dir.path(),
            "line 50\n",
            "line 50 updated\n",
        );

        let hunk_header = patch.lines().find(|line| line.starts_with("@@ "));
        assert_eq!(hunk_header, Some("@@ -50 +50 @@"));
    }

    #[test]
    fn test_try_parse_error_message() {
        let text = r#"{
  "error": {
    "message": "Your refresh token has already been used to generate a new access token. Please try signing in again.",
    "type": "invalid_request_error",
    "param": null,
    "code": "refresh_token_reused"
  }
}"#;
        let message = try_parse_error_message(text);
        assert_eq!(
            message,
            "Your refresh token has already been used to generate a new access token. Please try signing in again."
        );
    }

    #[test]
    fn test_try_parse_error_message_no_error() {
        let text = r#"{"message": "test"}"#;
        let message = try_parse_error_message(text);
        assert_eq!(message, r#"{"message": "test"}"#);
    }
}
