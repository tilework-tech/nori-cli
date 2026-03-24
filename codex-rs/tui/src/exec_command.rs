use std::path::Path;
use std::path::PathBuf;

use codex_core::parse_command::extract_shell_command;
use dirs::home_dir;
use shlex::try_join;

pub(crate) fn escape_command(command: &[String]) -> String {
    try_join(command.iter().map(String::as_str)).unwrap_or_else(|_| command.join(" "))
}

pub(crate) fn strip_bash_lc_and_escape(command: &[String]) -> String {
    if let Some((_, script)) = extract_shell_command(command) {
        return script.to_string();
    }
    // A single-element command is already a complete shell string (e.g. from
    // ACP "df -h --total | tail -1").  Return it verbatim so the syntax
    // highlighter sees real bash tokens instead of a shlex-quoted string.
    if command.len() == 1 {
        return command[0].clone();
    }
    escape_command(command)
}

/// If `path` is absolute and inside $HOME, return the part *after* the home
/// directory; otherwise, return the path as-is. Note if `path` is the homedir,
/// this will return and empty path.
pub(crate) fn relativize_to_home<P>(path: P) -> Option<PathBuf>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();
    if !path.is_absolute() {
        // If the path is not absolute, we can’t do anything with it.
        return None;
    }

    let home_dir = home_dir()?;
    let rel = path.strip_prefix(&home_dir).ok()?;
    Some(rel.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_command() {
        let args = vec!["foo".into(), "bar baz".into(), "weird&stuff".into()];
        let cmdline = escape_command(&args);
        assert_eq!(cmdline, "foo 'bar baz' 'weird&stuff'");
    }

    #[test]
    fn test_strip_bash_lc_and_escape() {
        // Test bash
        let args = vec!["bash".into(), "-lc".into(), "echo hello".into()];
        let cmdline = strip_bash_lc_and_escape(&args);
        assert_eq!(cmdline, "echo hello");

        // Test zsh
        let args = vec!["zsh".into(), "-lc".into(), "echo hello".into()];
        let cmdline = strip_bash_lc_and_escape(&args);
        assert_eq!(cmdline, "echo hello");

        // Test absolute path to zsh
        let args = vec!["/usr/bin/zsh".into(), "-lc".into(), "echo hello".into()];
        let cmdline = strip_bash_lc_and_escape(&args);
        assert_eq!(cmdline, "echo hello");

        // Test absolute path to bash
        let args = vec!["/bin/bash".into(), "-lc".into(), "echo hello".into()];
        let cmdline = strip_bash_lc_and_escape(&args);
        assert_eq!(cmdline, "echo hello");
    }

    #[test]
    fn single_element_command_with_metacharacters_not_quoted() {
        // ACP sends shell commands as a single string; it should NOT be
        // wrapped in quotes by shlex, or the syntax highlighter will treat
        // the entire command as a quoted string (one color).
        let args = vec!["df -h --total 2>/dev/null | tail -1".into()];
        let cmdline = strip_bash_lc_and_escape(&args);
        assert_eq!(cmdline, "df -h --total 2>/dev/null | tail -1");
    }

    #[test]
    fn multi_element_command_still_escaped() {
        let args = vec!["git".into(), "status".into()];
        let cmdline = strip_bash_lc_and_escape(&args);
        assert_eq!(cmdline, "git status");

        let args = vec!["foo".into(), "bar baz".into()];
        let cmdline = strip_bash_lc_and_escape(&args);
        assert_eq!(cmdline, "foo 'bar baz'");
    }
}
