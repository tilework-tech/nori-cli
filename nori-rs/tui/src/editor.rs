use std::io;
use std::path::Path;
use std::path::PathBuf;

/// Resolve which editor to launch by checking `$VISUAL`, then `$EDITOR`,
/// falling back to a platform default.
pub fn resolve_editor() -> String {
    resolve_editor_from(std::env::var("VISUAL").ok(), std::env::var("EDITOR").ok())
}

/// Inner resolution logic, testable without mutating env vars.
fn resolve_editor_from(visual: Option<String>, editor: Option<String>) -> String {
    if let Some(v) = visual
        && !v.is_empty()
    {
        return v;
    }
    if let Some(v) = editor
        && !v.is_empty()
    {
        return v;
    }
    #[cfg(unix)]
    {
        "vi".to_string()
    }
    #[cfg(windows)]
    {
        "notepad".to_string()
    }
}

/// Write `content` to a temporary file and return the path. The caller is
/// responsible for cleaning up the file after the editor exits.
pub fn write_temp_file(content: &str) -> io::Result<PathBuf> {
    let mut tmp = tempfile::Builder::new()
        .prefix("nori-editor-")
        .suffix(".md")
        .tempfile()?;
    io::Write::write_all(&mut tmp, content.as_bytes())?;
    // Persist so the file survives after the NamedTempFile is dropped.
    let (_, path) = tmp.keep().map_err(|e| e.error)?;
    Ok(path)
}

/// Read the content of `path`, remove the file, and return the text. Trailing
/// whitespace is preserved so the caller can decide how to handle it.
pub fn read_and_cleanup_temp_file(path: &Path) -> io::Result<String> {
    let content = std::fs::read_to_string(path)?;
    let _ = std::fs::remove_file(path);
    Ok(content)
}

/// Spawn the user's editor on `path`, blocking until it exits.
///
/// The caller must have already called `tui::restore()` before invoking this
/// so the terminal is in cooked mode.
pub fn spawn_editor(editor: &str, path: &Path) -> io::Result<std::process::ExitStatus> {
    #[cfg(unix)]
    {
        let path_str = path.display().to_string();
        let escaped_path = shlex::try_quote(&path_str)
            .unwrap_or(std::borrow::Cow::Owned(format!("'{}'", path.display())));
        std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("{editor} {escaped_path}"))
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()
    }
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .arg("/C")
            .arg(format!("{editor} \"{}\"", path.display()))
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_editor_prefers_visual_over_editor() {
        let result = resolve_editor_from(Some("code --wait".to_string()), Some("vim".to_string()));
        assert_eq!(result, "code --wait");
    }

    #[test]
    fn resolve_editor_falls_back_to_editor_when_visual_empty() {
        let result = resolve_editor_from(Some(String::new()), Some("nano".to_string()));
        assert_eq!(result, "nano");
    }

    #[test]
    fn resolve_editor_falls_back_to_editor_when_visual_unset() {
        let result = resolve_editor_from(None, Some("emacs".to_string()));
        assert_eq!(result, "emacs");
    }

    #[test]
    fn resolve_editor_falls_back_to_platform_default() {
        let result = resolve_editor_from(None, None);
        #[cfg(unix)]
        assert_eq!(result, "vi");
        #[cfg(windows)]
        assert_eq!(result, "notepad");
    }

    #[test]
    fn resolve_editor_falls_back_when_both_empty() {
        let result = resolve_editor_from(Some(String::new()), Some(String::new()));
        #[cfg(unix)]
        assert_eq!(result, "vi");
        #[cfg(windows)]
        assert_eq!(result, "notepad");
    }

    #[test]
    fn temp_file_round_trip_preserves_content() {
        let content = "Hello, world!\nThis is a test.\n\nMultiple lines.";
        let path = write_temp_file(content).expect("write_temp_file should succeed");

        assert!(path.exists(), "temp file should exist after write");
        assert!(
            path.to_string_lossy().contains("nori-editor-"),
            "temp file should have expected prefix"
        );
        assert!(
            path.to_string_lossy().ends_with(".md"),
            "temp file should have .md extension"
        );

        let read_back = read_and_cleanup_temp_file(&path).expect("read should succeed");
        assert_eq!(read_back, content);
        assert!(!path.exists(), "temp file should be cleaned up after read");
    }

    #[test]
    fn temp_file_round_trip_empty_content() {
        let path = write_temp_file("").expect("write_temp_file should succeed");
        let read_back = read_and_cleanup_temp_file(&path).expect("read should succeed");
        assert_eq!(read_back, "");
    }

    #[test]
    fn temp_file_round_trip_unicode_content() {
        let content = "日本語テスト 🎉\némojis and açcénts";
        let path = write_temp_file(content).expect("write_temp_file should succeed");
        let read_back = read_and_cleanup_temp_file(&path).expect("read should succeed");
        assert_eq!(read_back, content);
    }
}
