use codex_protocol::custom_prompts::CustomPrompt;
use codex_protocol::custom_prompts::CustomPromptKind;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use tokio::fs;

/// Return the default prompts directory: `$CODEX_HOME/prompts`.
/// If `CODEX_HOME` cannot be resolved, returns `None`.
pub fn default_prompts_dir() -> Option<PathBuf> {
    crate::config::find_codex_home()
        .ok()
        .map(|home| home.join("prompts"))
}

/// Discover prompt files in the given directory, returning entries sorted by name.
/// Non-files are ignored. If the directory does not exist or cannot be read, returns empty.
pub async fn discover_prompts_in(dir: &Path) -> Vec<CustomPrompt> {
    discover_prompts_in_excluding(dir, &HashSet::new()).await
}

/// Discover prompt files in the given directory, excluding any with names in `exclude`.
/// Returns entries sorted by name. Non-files are ignored. Missing/unreadable dir yields empty.
pub async fn discover_prompts_in_excluding(
    dir: &Path,
    exclude: &HashSet<String>,
) -> Vec<CustomPrompt> {
    let mut out: Vec<CustomPrompt> = Vec::new();
    let mut entries = match fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(_) => return out,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let is_file_like = fs::metadata(&path)
            .await
            .map(|m| m.is_file())
            .unwrap_or(false);
        if !is_file_like {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .map(str::to_ascii_lowercase);
        let kind = match ext.as_deref() {
            Some("md") => CustomPromptKind::Markdown,
            Some("sh") => CustomPromptKind::Script {
                interpreter: "bash".to_string(),
            },
            Some("py") => CustomPromptKind::Script {
                interpreter: "python3".to_string(),
            },
            Some("js") => CustomPromptKind::Script {
                interpreter: "node".to_string(),
            },
            _ => continue,
        };
        let Some(name) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        if exclude.contains(&name) {
            continue;
        }
        let (content, description, argument_hint) = match &kind {
            CustomPromptKind::Markdown => {
                let raw = match fs::read_to_string(&path).await {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let (desc, hint, body) = parse_frontmatter(&raw);
                (body, desc, hint)
            }
            CustomPromptKind::Script { .. } => (String::new(), None, None),
        };
        out.push(CustomPrompt {
            name,
            path,
            content,
            description,
            argument_hint,
            kind,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Parse optional YAML-like frontmatter at the beginning of `content`.
/// Supported keys:
/// - `description`: short description shown in the slash popup
/// - `argument-hint` or `argument_hint`: brief hint string shown after the description
///   Returns (description, argument_hint, body_without_frontmatter).
fn parse_frontmatter(content: &str) -> (Option<String>, Option<String>, String) {
    let mut segments = content.split_inclusive('\n');
    let Some(first_segment) = segments.next() else {
        return (None, None, String::new());
    };
    let first_line = first_segment.trim_end_matches(['\r', '\n']);
    if first_line.trim() != "---" {
        return (None, None, content.to_string());
    }

    let mut desc: Option<String> = None;
    let mut hint: Option<String> = None;
    let mut frontmatter_closed = false;
    let mut consumed = first_segment.len();

    for segment in segments {
        let line = segment.trim_end_matches(['\r', '\n']);
        let trimmed = line.trim();

        if trimmed == "---" {
            frontmatter_closed = true;
            consumed += segment.len();
            break;
        }

        if trimmed.is_empty() || trimmed.starts_with('#') {
            consumed += segment.len();
            continue;
        }

        if let Some((k, v)) = trimmed.split_once(':') {
            let key = k.trim().to_ascii_lowercase();
            let mut val = v.trim().to_string();
            if val.len() >= 2 {
                let bytes = val.as_bytes();
                let first = bytes[0];
                let last = bytes[bytes.len() - 1];
                if (first == b'\"' && last == b'\"') || (first == b'\'' && last == b'\'') {
                    val = val[1..val.len().saturating_sub(1)].to_string();
                }
            }
            match key.as_str() {
                "description" => desc = Some(val),
                "argument-hint" | "argument_hint" => hint = Some(val),
                _ => {}
            }
        }

        consumed += segment.len();
    }

    if !frontmatter_closed {
        // Unterminated frontmatter: treat input as-is.
        return (None, None, content.to_string());
    }

    let body = if consumed >= content.len() {
        String::new()
    } else {
        content[consumed..].to_string()
    };
    (desc, hint, body)
}

/// Execute a script prompt and return its stdout on success.
///
/// The script is run via the interpreter specified in `prompt.kind` (e.g.
/// `bash`, `python3`, `node`). Positional arguments are passed through.
/// Returns `Ok(stdout)` on zero exit, or `Err(message)` on non-zero exit
/// or timeout.
pub async fn execute_script(
    prompt: &CustomPrompt,
    args: &[String],
    timeout: Duration,
) -> Result<String, String> {
    let CustomPromptKind::Script { ref interpreter } = prompt.kind else {
        return Err("not a script prompt".to_string());
    };

    let mut cmd = tokio::process::Command::new(interpreter);
    cmd.arg(&prompt.path);
    cmd.args(args);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.stdin(std::process::Stdio::null());
    cmd.kill_on_drop(true);

    let child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn {interpreter}: {e}"))?;

    let result = tokio::time::timeout(timeout, child.wait_with_output()).await;

    match result {
        Ok(Ok(output)) => {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                Ok(stdout)
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let code = output
                    .status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                Err(format!(
                    "Script '{}' failed with exit code {code}: {stderr}",
                    prompt.name
                ))
            }
        }
        Ok(Err(e)) => Err(format!("Script '{}' I/O error: {e}", prompt.name)),
        Err(_) => Err(format!(
            "Script '{}' timed out after {:.1}s",
            prompt.name,
            timeout.as_secs_f64()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn empty_when_dir_missing() {
        let tmp = tempdir().expect("create TempDir");
        let missing = tmp.path().join("nope");
        let found = discover_prompts_in(&missing).await;
        assert!(found.is_empty());
    }

    #[tokio::test]
    async fn discovers_and_sorts_files() {
        let tmp = tempdir().expect("create TempDir");
        let dir = tmp.path();
        fs::write(dir.join("b.md"), b"b").unwrap();
        fs::write(dir.join("a.md"), b"a").unwrap();
        fs::create_dir(dir.join("subdir")).unwrap();
        let found = discover_prompts_in(dir).await;
        let names: Vec<String> = found.into_iter().map(|e| e.name).collect();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[tokio::test]
    async fn excludes_builtins() {
        let tmp = tempdir().expect("create TempDir");
        let dir = tmp.path();
        fs::write(dir.join("init.md"), b"ignored").unwrap();
        fs::write(dir.join("foo.md"), b"ok").unwrap();
        let mut exclude = HashSet::new();
        exclude.insert("init".to_string());
        let found = discover_prompts_in_excluding(dir, &exclude).await;
        let names: Vec<String> = found.into_iter().map(|e| e.name).collect();
        assert_eq!(names, vec!["foo"]);
    }

    #[tokio::test]
    async fn skips_non_utf8_files() {
        let tmp = tempdir().expect("create TempDir");
        let dir = tmp.path();
        // Valid UTF-8 file
        fs::write(dir.join("good.md"), b"hello").unwrap();
        // Invalid UTF-8 content in .md file (e.g., lone 0xFF byte)
        fs::write(dir.join("bad.md"), vec![0xFF, 0xFE, b'\n']).unwrap();
        let found = discover_prompts_in(dir).await;
        let names: Vec<String> = found.into_iter().map(|e| e.name).collect();
        assert_eq!(names, vec!["good"]);
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn discovers_symlinked_md_files() {
        let tmp = tempdir().expect("create TempDir");
        let dir = tmp.path();

        // Create a real file
        fs::write(dir.join("real.md"), b"real content").unwrap();

        // Create a symlink to the real file
        std::os::unix::fs::symlink(dir.join("real.md"), dir.join("link.md")).unwrap();

        let found = discover_prompts_in(dir).await;
        let names: Vec<String> = found.into_iter().map(|e| e.name).collect();

        // Both real and link should be discovered, sorted alphabetically
        assert_eq!(names, vec!["link", "real"]);
    }

    #[tokio::test]
    async fn parses_frontmatter_and_strips_from_body() {
        let tmp = tempdir().expect("create TempDir");
        let dir = tmp.path();
        let file = dir.join("withmeta.md");
        let text = "---\nname: ignored\ndescription: \"Quick review command\"\nargument-hint: \"[file] [priority]\"\n---\nActual body with $1 and $ARGUMENTS";
        fs::write(&file, text).unwrap();

        let found = discover_prompts_in(dir).await;
        assert_eq!(found.len(), 1);
        let p = &found[0];
        assert_eq!(p.name, "withmeta");
        assert_eq!(p.description.as_deref(), Some("Quick review command"));
        assert_eq!(p.argument_hint.as_deref(), Some("[file] [priority]"));
        // Body should not include the frontmatter delimiters.
        assert_eq!(p.content, "Actual body with $1 and $ARGUMENTS");
    }

    #[test]
    fn parse_frontmatter_preserves_body_newlines() {
        let content = "---\r\ndescription: \"Line endings\"\r\nargument_hint: \"[arg]\"\r\n---\r\nFirst line\r\nSecond line\r\n";
        let (desc, hint, body) = parse_frontmatter(content);
        assert_eq!(desc.as_deref(), Some("Line endings"));
        assert_eq!(hint.as_deref(), Some("[arg]"));
        assert_eq!(body, "First line\r\nSecond line\r\n");
    }

    // ========================================================================
    // Script discovery tests
    // ========================================================================

    #[tokio::test]
    async fn discovers_script_files_alongside_markdown() {
        use codex_protocol::custom_prompts::CustomPromptKind;

        let tmp = tempdir().expect("create TempDir");
        let dir = tmp.path();
        fs::write(dir.join("alpha.md"), b"markdown body").unwrap();
        fs::write(dir.join("beta.sh"), b"#!/bin/bash\necho hi").unwrap();
        fs::write(dir.join("gamma.py"), b"print('hi')").unwrap();
        fs::write(dir.join("delta.js"), b"console.log('hi')").unwrap();
        // .txt should still be ignored
        fs::write(dir.join("ignore.txt"), b"nope").unwrap();

        let found = discover_prompts_in(dir).await;
        let names: Vec<&str> = found.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "beta", "delta", "gamma"]);

        // Verify kinds
        let alpha = found.iter().find(|p| p.name == "alpha").unwrap();
        assert_eq!(alpha.kind, CustomPromptKind::Markdown);

        let beta = found.iter().find(|p| p.name == "beta").unwrap();
        assert!(
            matches!(beta.kind, CustomPromptKind::Script { ref interpreter } if interpreter == "bash")
        );

        let gamma = found.iter().find(|p| p.name == "gamma").unwrap();
        assert!(
            matches!(gamma.kind, CustomPromptKind::Script { ref interpreter } if interpreter == "python3")
        );

        let delta = found.iter().find(|p| p.name == "delta").unwrap();
        assert!(
            matches!(delta.kind, CustomPromptKind::Script { ref interpreter } if interpreter == "node")
        );
    }

    #[tokio::test]
    async fn script_prompts_have_empty_content_at_discovery() {
        let tmp = tempdir().expect("create TempDir");
        let dir = tmp.path();
        fs::write(dir.join("myscript.sh"), b"#!/bin/bash\necho hello").unwrap();

        let found = discover_prompts_in(dir).await;
        assert_eq!(found.len(), 1);
        assert!(found[0].content.is_empty());
    }

    // ========================================================================
    // Script execution tests
    // ========================================================================

    #[tokio::test]
    async fn execute_script_captures_stdout() {
        use codex_protocol::custom_prompts::CustomPromptKind;
        use std::time::Duration;

        let tmp = tempdir().expect("create TempDir");
        let script_path = tmp.path().join("greet.sh");
        fs::write(&script_path, "#!/bin/bash\necho 'hello world'").unwrap();

        let prompt = CustomPrompt {
            name: "greet".to_string(),
            path: script_path,
            content: String::new(),
            description: None,
            argument_hint: None,
            kind: CustomPromptKind::Script {
                interpreter: "bash".to_string(),
            },
        };

        let result = execute_script(&prompt, &[], Duration::from_secs(5)).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().trim(), "hello world");
    }

    #[tokio::test]
    async fn execute_script_returns_error_on_nonzero_exit() {
        use codex_protocol::custom_prompts::CustomPromptKind;
        use std::time::Duration;

        let tmp = tempdir().expect("create TempDir");
        let script_path = tmp.path().join("fail.sh");
        fs::write(&script_path, "#!/bin/bash\necho 'oops' >&2\nexit 1").unwrap();

        let prompt = CustomPrompt {
            name: "fail".to_string(),
            path: script_path,
            content: String::new(),
            description: None,
            argument_hint: None,
            kind: CustomPromptKind::Script {
                interpreter: "bash".to_string(),
            },
        };

        let result = execute_script(&prompt, &[], Duration::from_secs(5)).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("oops"));
        assert!(err.contains("exit code"));
    }

    #[tokio::test]
    async fn execute_script_passes_positional_args() {
        use codex_protocol::custom_prompts::CustomPromptKind;
        use std::time::Duration;

        let tmp = tempdir().expect("create TempDir");
        let script_path = tmp.path().join("echo_args.sh");
        fs::write(&script_path, "#!/bin/bash\necho \"$1 $2\"").unwrap();

        let prompt = CustomPrompt {
            name: "echo_args".to_string(),
            path: script_path,
            content: String::new(),
            description: None,
            argument_hint: None,
            kind: CustomPromptKind::Script {
                interpreter: "bash".to_string(),
            },
        };

        let result = execute_script(
            &prompt,
            &["foo".to_string(), "bar".to_string()],
            Duration::from_secs(5),
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().trim(), "foo bar");
    }

    #[tokio::test]
    async fn execute_script_returns_empty_string_on_no_output() {
        use codex_protocol::custom_prompts::CustomPromptKind;
        use std::time::Duration;

        let tmp = tempdir().expect("create TempDir");
        let script_path = tmp.path().join("silent.sh");
        fs::write(&script_path, "#!/bin/bash\n# no output").unwrap();

        let prompt = CustomPrompt {
            name: "silent".to_string(),
            path: script_path,
            content: String::new(),
            description: None,
            argument_hint: None,
            kind: CustomPromptKind::Script {
                interpreter: "bash".to_string(),
            },
        };

        let result = execute_script(&prompt, &[], Duration::from_secs(5)).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn execute_script_times_out() {
        use codex_protocol::custom_prompts::CustomPromptKind;
        use std::time::Duration;

        let tmp = tempdir().expect("create TempDir");
        let script_path = tmp.path().join("hang.sh");
        fs::write(&script_path, "#!/bin/bash\nsleep 60").unwrap();

        let prompt = CustomPrompt {
            name: "hang".to_string(),
            path: script_path,
            content: String::new(),
            description: None,
            argument_hint: None,
            kind: CustomPromptKind::Script {
                interpreter: "bash".to_string(),
            },
        };

        let result = execute_script(&prompt, &[], Duration::from_millis(100)).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_lowercase().contains("timed out"));
    }
}
