use super::*;
use assert_matches::assert_matches;
use pretty_assertions::assert_eq;
use std::fs;
use std::path::PathBuf;
use std::string::ToString;
use tempfile::tempdir;

/// Helper to construct a patch with the given body.
fn wrap_patch(body: &str) -> String {
    format!("*** Begin Patch\n{body}\n*** End Patch")
}

fn strs_to_strings(strs: &[&str]) -> Vec<String> {
    strs.iter().map(ToString::to_string).collect()
}

// Test helpers to reduce repetition when building bash -lc heredoc scripts
fn args_bash(script: &str) -> Vec<String> {
    strs_to_strings(&["bash", "-lc", script])
}

fn args_powershell(script: &str) -> Vec<String> {
    strs_to_strings(&["powershell.exe", "-Command", script])
}

fn args_powershell_no_profile(script: &str) -> Vec<String> {
    strs_to_strings(&["powershell.exe", "-NoProfile", "-Command", script])
}

fn args_pwsh(script: &str) -> Vec<String> {
    strs_to_strings(&["pwsh", "-NoProfile", "-Command", script])
}

fn args_cmd(script: &str) -> Vec<String> {
    strs_to_strings(&["cmd.exe", "/c", script])
}

fn heredoc_script(prefix: &str) -> String {
    format!(
        "{prefix}apply_patch <<'PATCH'\n*** Begin Patch\n*** Add File: foo\n+hi\n*** End Patch\nPATCH"
    )
}

fn heredoc_script_ps(prefix: &str, suffix: &str) -> String {
    format!(
        "{prefix}apply_patch <<'PATCH'\n*** Begin Patch\n*** Add File: foo\n+hi\n*** End Patch\nPATCH{suffix}"
    )
}

fn expected_single_add() -> Vec<Hunk> {
    vec![Hunk::AddFile {
        path: PathBuf::from("foo"),
        contents: "hi\n".to_string(),
    }]
}

fn assert_match_args(args: Vec<String>, expected_workdir: Option<&str>) {
    match maybe_parse_apply_patch(&args) {
        MaybeApplyPatch::Body(ApplyPatchArgs { hunks, workdir, .. }) => {
            assert_eq!(workdir.as_deref(), expected_workdir);
            assert_eq!(hunks, expected_single_add());
        }
        result => panic!("expected MaybeApplyPatch::Body got {result:?}"),
    }
}

fn assert_match(script: &str, expected_workdir: Option<&str>) {
    let args = args_bash(script);
    assert_match_args(args, expected_workdir);
}

fn assert_not_match(script: &str) {
    let args = args_bash(script);
    assert_matches!(
        maybe_parse_apply_patch(&args),
        MaybeApplyPatch::NotApplyPatch
    );
}

#[test]
fn test_implicit_patch_single_arg_is_error() {
    let patch = "*** Begin Patch\n*** Add File: foo\n+hi\n*** End Patch".to_string();
    let args = vec![patch];
    let dir = tempdir().unwrap();
    assert_matches!(
        maybe_parse_apply_patch_verified(&args, dir.path()),
        MaybeApplyPatchVerified::CorrectnessError(ApplyPatchError::ImplicitInvocation)
    );
}

#[test]
fn test_implicit_patch_bash_script_is_error() {
    let script = "*** Begin Patch\n*** Add File: foo\n+hi\n*** End Patch";
    let args = args_bash(script);
    let dir = tempdir().unwrap();
    assert_matches!(
        maybe_parse_apply_patch_verified(&args, dir.path()),
        MaybeApplyPatchVerified::CorrectnessError(ApplyPatchError::ImplicitInvocation)
    );
}

#[test]
fn test_literal() {
    let args = strs_to_strings(&[
        "apply_patch",
        r#"*** Begin Patch
*** Add File: foo
+hi
*** End Patch
"#,
    ]);

    match maybe_parse_apply_patch(&args) {
        MaybeApplyPatch::Body(ApplyPatchArgs { hunks, .. }) => {
            assert_eq!(
                hunks,
                vec![Hunk::AddFile {
                    path: PathBuf::from("foo"),
                    contents: "hi\n".to_string()
                }]
            );
        }
        result => panic!("expected MaybeApplyPatch::Body got {result:?}"),
    }
}

#[test]
fn test_literal_applypatch() {
    let args = strs_to_strings(&[
        "applypatch",
        r#"*** Begin Patch
*** Add File: foo
+hi
*** End Patch
"#,
    ]);

    match maybe_parse_apply_patch(&args) {
        MaybeApplyPatch::Body(ApplyPatchArgs { hunks, .. }) => {
            assert_eq!(
                hunks,
                vec![Hunk::AddFile {
                    path: PathBuf::from("foo"),
                    contents: "hi\n".to_string()
                }]
            );
        }
        result => panic!("expected MaybeApplyPatch::Body got {result:?}"),
    }
}

#[test]
fn test_heredoc() {
    assert_match(&heredoc_script(""), None);
}

#[test]
fn test_heredoc_applypatch() {
    let args = strs_to_strings(&[
        "bash",
        "-lc",
        r#"applypatch <<'PATCH'
*** Begin Patch
*** Add File: foo
+hi
*** End Patch
PATCH"#,
    ]);

    match maybe_parse_apply_patch(&args) {
        MaybeApplyPatch::Body(ApplyPatchArgs { hunks, workdir, .. }) => {
            assert_eq!(workdir, None);
            assert_eq!(
                hunks,
                vec![Hunk::AddFile {
                    path: PathBuf::from("foo"),
                    contents: "hi\n".to_string()
                }]
            );
        }
        result => panic!("expected MaybeApplyPatch::Body got {result:?}"),
    }
}

#[test]
fn test_powershell_heredoc() {
    let script = heredoc_script("");
    assert_match_args(args_powershell(&script), None);
}
#[test]
fn test_powershell_heredoc_no_profile() {
    let script = heredoc_script("");
    assert_match_args(args_powershell_no_profile(&script), None);
}
#[test]
fn test_pwsh_heredoc() {
    let script = heredoc_script("");
    assert_match_args(args_pwsh(&script), None);
}

#[test]
fn test_cmd_heredoc_with_cd() {
    let script = heredoc_script("cd foo && ");
    assert_match_args(args_cmd(&script), Some("foo"));
}

#[test]
fn test_heredoc_with_leading_cd() {
    assert_match(&heredoc_script("cd foo && "), Some("foo"));
}

#[test]
fn test_cd_with_semicolon_is_ignored() {
    assert_not_match(&heredoc_script("cd foo; "));
}

#[test]
fn test_cd_or_apply_patch_is_ignored() {
    assert_not_match(&heredoc_script("cd bar || "));
}

#[test]
fn test_cd_pipe_apply_patch_is_ignored() {
    assert_not_match(&heredoc_script("cd bar | "));
}

#[test]
fn test_cd_single_quoted_path_with_spaces() {
    assert_match(&heredoc_script("cd 'foo bar' && "), Some("foo bar"));
}

#[test]
fn test_cd_double_quoted_path_with_spaces() {
    assert_match(&heredoc_script("cd \"foo bar\" && "), Some("foo bar"));
}

#[test]
fn test_echo_and_apply_patch_is_ignored() {
    assert_not_match(&heredoc_script("echo foo && "));
}

#[test]
fn test_apply_patch_with_arg_is_ignored() {
    let script =
        "apply_patch foo <<'PATCH'\n*** Begin Patch\n*** Add File: foo\n+hi\n*** End Patch\nPATCH";
    assert_not_match(script);
}

#[test]
fn test_double_cd_then_apply_patch_is_ignored() {
    assert_not_match(&heredoc_script("cd foo && cd bar && "));
}

#[test]
fn test_cd_two_args_is_ignored() {
    assert_not_match(&heredoc_script("cd foo bar && "));
}

#[test]
fn test_cd_then_apply_patch_then_extra_is_ignored() {
    let script = heredoc_script_ps("cd bar && ", " && echo done");
    assert_not_match(&script);
}

#[test]
fn test_echo_then_cd_and_apply_patch_is_ignored() {
    // Ensure preceding commands before the `cd && apply_patch <<...` sequence do not match.
    assert_not_match(&heredoc_script("echo foo; cd bar && "));
}

#[test]
fn test_add_file_hunk_creates_file_with_contents() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("add.txt");
    let patch = wrap_patch(&format!(
        r#"*** Add File: {}
+ab
+cd"#,
        path.display()
    ));
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    apply_patch(&patch, &mut stdout, &mut stderr).unwrap();
    // Verify expected stdout and stderr outputs.
    let stdout_str = String::from_utf8(stdout).unwrap();
    let stderr_str = String::from_utf8(stderr).unwrap();
    let expected_out = format!(
        "Success. Updated the following files:\nA {}\n",
        path.display()
    );
    assert_eq!(stdout_str, expected_out);
    assert_eq!(stderr_str, "");
    let contents = fs::read_to_string(path).unwrap();
    assert_eq!(contents, "ab\ncd\n");
}

#[test]
fn test_delete_file_hunk_removes_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("del.txt");
    fs::write(&path, "x").unwrap();
    let patch = wrap_patch(&format!("*** Delete File: {}", path.display()));
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    apply_patch(&patch, &mut stdout, &mut stderr).unwrap();
    let stdout_str = String::from_utf8(stdout).unwrap();
    let stderr_str = String::from_utf8(stderr).unwrap();
    let expected_out = format!(
        "Success. Updated the following files:\nD {}\n",
        path.display()
    );
    assert_eq!(stdout_str, expected_out);
    assert_eq!(stderr_str, "");
    assert!(!path.exists());
}

#[test]
fn test_update_file_hunk_modifies_content() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("update.txt");
    fs::write(&path, "foo\nbar\n").unwrap();
    let patch = wrap_patch(&format!(
        r#"*** Update File: {}
@@
 foo
-bar
+baz"#,
        path.display()
    ));
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    apply_patch(&patch, &mut stdout, &mut stderr).unwrap();
    // Validate modified file contents and expected stdout/stderr.
    let stdout_str = String::from_utf8(stdout).unwrap();
    let stderr_str = String::from_utf8(stderr).unwrap();
    let expected_out = format!(
        "Success. Updated the following files:\nM {}\n",
        path.display()
    );
    assert_eq!(stdout_str, expected_out);
    assert_eq!(stderr_str, "");
    let contents = fs::read_to_string(&path).unwrap();
    assert_eq!(contents, "foo\nbaz\n");
}

#[test]
fn test_update_file_hunk_can_move_file() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src.txt");
    let dest = dir.path().join("dst.txt");
    fs::write(&src, "line\n").unwrap();
    let patch = wrap_patch(&format!(
        r#"*** Update File: {}
*** Move to: {}
@@
-line
+line2"#,
        src.display(),
        dest.display()
    ));
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    apply_patch(&patch, &mut stdout, &mut stderr).unwrap();
    // Validate move semantics and expected stdout/stderr.
    let stdout_str = String::from_utf8(stdout).unwrap();
    let stderr_str = String::from_utf8(stderr).unwrap();
    let expected_out = format!(
        "Success. Updated the following files:\nM {}\n",
        dest.display()
    );
    assert_eq!(stdout_str, expected_out);
    assert_eq!(stderr_str, "");
    assert!(!src.exists());
    let contents = fs::read_to_string(&dest).unwrap();
    assert_eq!(contents, "line2\n");
}

/// Verify that a single `Update File` hunk with multiple change chunks can update different
/// parts of a file and that the file is listed only once in the summary.
#[test]
fn test_multiple_update_chunks_apply_to_single_file() {
    // Start with a file containing four lines.
    let dir = tempdir().unwrap();
    let path = dir.path().join("multi.txt");
    fs::write(&path, "foo\nbar\nbaz\nqux\n").unwrap();
    // Construct an update patch with two separate change chunks.
    // The first chunk uses the line `foo` as context and transforms `bar` into `BAR`.
    // The second chunk uses `baz` as context and transforms `qux` into `QUX`.
    let patch = wrap_patch(&format!(
        r#"*** Update File: {}
@@
 foo
-bar
+BAR
@@
 baz
-qux
+QUX"#,
        path.display()
    ));
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    apply_patch(&patch, &mut stdout, &mut stderr).unwrap();
    let stdout_str = String::from_utf8(stdout).unwrap();
    let stderr_str = String::from_utf8(stderr).unwrap();
    let expected_out = format!(
        "Success. Updated the following files:\nM {}\n",
        path.display()
    );
    assert_eq!(stdout_str, expected_out);
    assert_eq!(stderr_str, "");
    let contents = fs::read_to_string(&path).unwrap();
    assert_eq!(contents, "foo\nBAR\nbaz\nQUX\n");
}

/// A more involved `Update File` hunk that exercises additions, deletions and
/// replacements in separate chunks that appear in non-adjacent parts of the
/// file.  Verifies that all edits are applied and that the summary lists the
/// file only once.
#[test]
fn test_update_file_hunk_interleaved_changes() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("interleaved.txt");

    // Original file: six numbered lines.
    fs::write(&path, "a\nb\nc\nd\ne\nf\n").unwrap();

    // Patch performs:
    //  * Replace `b` -> `B`
    //  * Replace `e` -> `E` (using surrounding context)
    //  * Append new line `g` at the end-of-file
    let patch = wrap_patch(&format!(
        r#"*** Update File: {}
@@
 a
-b
+B
@@
 c
 d
-e
+E
@@
 f
+g
*** End of File"#,
        path.display()
    ));

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    apply_patch(&patch, &mut stdout, &mut stderr).unwrap();

    let stdout_str = String::from_utf8(stdout).unwrap();
    let stderr_str = String::from_utf8(stderr).unwrap();

    let expected_out = format!(
        "Success. Updated the following files:\nM {}\n",
        path.display()
    );
    assert_eq!(stdout_str, expected_out);
    assert_eq!(stderr_str, "");

    let contents = fs::read_to_string(&path).unwrap();
    assert_eq!(contents, "a\nB\nc\nd\nE\nf\ng\n");
}

#[test]
fn test_pure_addition_chunk_followed_by_removal() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("panic.txt");
    fs::write(&path, "line1\nline2\nline3\n").unwrap();
    let patch = wrap_patch(&format!(
        r#"*** Update File: {}
@@
+after-context
+second-line
@@
 line1
-line2
-line3
+line2-replacement"#,
        path.display()
    ));
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    apply_patch(&patch, &mut stdout, &mut stderr).unwrap();
    let contents = fs::read_to_string(path).unwrap();
    assert_eq!(
        contents,
        "line1\nline2-replacement\nafter-context\nsecond-line\n"
    );
}

/// Ensure that patches authored with ASCII characters can update lines that
/// contain typographic Unicode punctuation (e.g. EN DASH, NON-BREAKING
/// HYPHEN). Historically `git apply` succeeds in such scenarios but our
/// internal matcher failed requiring an exact byte-for-byte match.  The
/// fuzzy-matching pass that normalises common punctuation should now bridge
/// the gap.
#[test]
fn test_update_line_with_unicode_dash() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("unicode.py");

    // Original line contains EN DASH (\u{2013}) and NON-BREAKING HYPHEN (\u{2011}).
    let original = "import asyncio  # local import \u{2013} avoids top\u{2011}level dep\n";
    std::fs::write(&path, original).unwrap();

    // Patch uses plain ASCII dash / hyphen.
    let patch = wrap_patch(&format!(
        r#"*** Update File: {}
@@
-import asyncio  # local import - avoids top-level dep
+import asyncio  # HELLO"#,
        path.display()
    ));

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    apply_patch(&patch, &mut stdout, &mut stderr).unwrap();

    // File should now contain the replaced comment.
    let expected = "import asyncio  # HELLO\n";
    let contents = std::fs::read_to_string(&path).unwrap();
    assert_eq!(contents, expected);

    // Ensure success summary lists the file as modified.
    let stdout_str = String::from_utf8(stdout).unwrap();
    let expected_out = format!(
        "Success. Updated the following files:\nM {}\n",
        path.display()
    );
    assert_eq!(stdout_str, expected_out);

    // No stderr expected.
    assert_eq!(String::from_utf8(stderr).unwrap(), "");
}

#[test]
fn test_unified_diff() {
    // Start with a file containing four lines.
    let dir = tempdir().unwrap();
    let path = dir.path().join("multi.txt");
    fs::write(&path, "foo\nbar\nbaz\nqux\n").unwrap();
    let patch = wrap_patch(&format!(
        r#"*** Update File: {}
@@
 foo
-bar
+BAR
@@
 baz
-qux
+QUX"#,
        path.display()
    ));
    let patch = parse_patch(&patch).unwrap();

    let update_file_chunks = match patch.hunks.as_slice() {
        [Hunk::UpdateFile { chunks, .. }] => chunks,
        _ => panic!("Expected a single UpdateFile hunk"),
    };
    let diff = unified_diff_from_chunks(&path, update_file_chunks).unwrap();
    let expected_diff = r#"@@ -1,4 +1,4 @@
 foo
-bar
+BAR
 baz
-qux
+QUX
"#;
    let expected = ApplyPatchFileUpdate {
        unified_diff: expected_diff.to_string(),
        content: "foo\nBAR\nbaz\nQUX\n".to_string(),
    };
    assert_eq!(expected, diff);
}

#[test]
fn test_unified_diff_first_line_replacement() {
    // Replace the very first line of the file.
    let dir = tempdir().unwrap();
    let path = dir.path().join("first.txt");
    fs::write(&path, "foo\nbar\nbaz\n").unwrap();

    let patch = wrap_patch(&format!(
        r#"*** Update File: {}
@@
-foo
+FOO
 bar
"#,
        path.display()
    ));

    let patch = parse_patch(&patch).unwrap();
    let chunks = match patch.hunks.as_slice() {
        [Hunk::UpdateFile { chunks, .. }] => chunks,
        _ => panic!("Expected a single UpdateFile hunk"),
    };

    let diff = unified_diff_from_chunks(&path, chunks).unwrap();
    let expected_diff = r#"@@ -1,2 +1,2 @@
-foo
+FOO
 bar
"#;
    let expected = ApplyPatchFileUpdate {
        unified_diff: expected_diff.to_string(),
        content: "FOO\nbar\nbaz\n".to_string(),
    };
    assert_eq!(expected, diff);
}

#[test]
fn test_unified_diff_last_line_replacement() {
    // Replace the very last line of the file.
    let dir = tempdir().unwrap();
    let path = dir.path().join("last.txt");
    fs::write(&path, "foo\nbar\nbaz\n").unwrap();

    let patch = wrap_patch(&format!(
        r#"*** Update File: {}
@@
 foo
 bar
-baz
+BAZ
"#,
        path.display()
    ));

    let patch = parse_patch(&patch).unwrap();
    let chunks = match patch.hunks.as_slice() {
        [Hunk::UpdateFile { chunks, .. }] => chunks,
        _ => panic!("Expected a single UpdateFile hunk"),
    };

    let diff = unified_diff_from_chunks(&path, chunks).unwrap();
    let expected_diff = r#"@@ -2,2 +2,2 @@
 bar
-baz
+BAZ
"#;
    let expected = ApplyPatchFileUpdate {
        unified_diff: expected_diff.to_string(),
        content: "foo\nbar\nBAZ\n".to_string(),
    };
    assert_eq!(expected, diff);
}

#[test]
fn test_unified_diff_insert_at_eof() {
    // Insert a new line at end-of-file.
    let dir = tempdir().unwrap();
    let path = dir.path().join("insert.txt");
    fs::write(&path, "foo\nbar\nbaz\n").unwrap();

    let patch = wrap_patch(&format!(
        r#"*** Update File: {}
@@
+quux
*** End of File
"#,
        path.display()
    ));

    let patch = parse_patch(&patch).unwrap();
    let chunks = match patch.hunks.as_slice() {
        [Hunk::UpdateFile { chunks, .. }] => chunks,
        _ => panic!("Expected a single UpdateFile hunk"),
    };

    let diff = unified_diff_from_chunks(&path, chunks).unwrap();
    let expected_diff = r#"@@ -3 +3,2 @@
 baz
+quux
"#;
    let expected = ApplyPatchFileUpdate {
        unified_diff: expected_diff.to_string(),
        content: "foo\nbar\nbaz\nquux\n".to_string(),
    };
    assert_eq!(expected, diff);
}

#[test]
fn test_unified_diff_interleaved_changes() {
    // Original file with six lines.
    let dir = tempdir().unwrap();
    let path = dir.path().join("interleaved.txt");
    fs::write(&path, "a\nb\nc\nd\ne\nf\n").unwrap();

    // Patch replaces two separate lines and appends a new one at EOF using
    // three distinct chunks.
    let patch_body = format!(
        r#"*** Update File: {}
@@
 a
-b
+B
@@
 d
-e
+E
@@
 f
+g
*** End of File"#,
        path.display()
    );
    let patch = wrap_patch(&patch_body);

    // Extract chunks then build the unified diff.
    let parsed = parse_patch(&patch).unwrap();
    let chunks = match parsed.hunks.as_slice() {
        [Hunk::UpdateFile { chunks, .. }] => chunks,
        _ => panic!("Expected a single UpdateFile hunk"),
    };

    let diff = unified_diff_from_chunks(&path, chunks).unwrap();

    let expected_diff = r#"@@ -1,6 +1,7 @@
 a
-b
+B
 c
 d
-e
+E
 f
+g
"#;

    let expected = ApplyPatchFileUpdate {
        unified_diff: expected_diff.to_string(),
        content: "a\nB\nc\nd\nE\nf\ng\n".to_string(),
    };

    assert_eq!(expected, diff);

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    apply_patch(&patch, &mut stdout, &mut stderr).unwrap();
    let contents = fs::read_to_string(path).unwrap();
    assert_eq!(
        contents,
        r#"a
B
c
d
E
f
g
"#
    );
}

#[test]
fn test_apply_patch_should_resolve_absolute_paths_in_cwd() {
    let session_dir = tempdir().unwrap();
    let relative_path = "source.txt";

    // Note that we need this file to exist for the patch to be "verified"
    // and parsed correctly.
    let session_file_path = session_dir.path().join(relative_path);
    fs::write(&session_file_path, "session directory content\n").unwrap();

    let argv = vec![
        "apply_patch".to_string(),
        r#"*** Begin Patch
*** Update File: source.txt
@@
-session directory content
+updated session directory content
*** End Patch"#
            .to_string(),
    ];

    let result = maybe_parse_apply_patch_verified(&argv, session_dir.path());

    // Verify the patch contents - as otherwise we may have pulled contents
    // from the wrong file (as we're using relative paths)
    assert_eq!(
        result,
        MaybeApplyPatchVerified::Body(ApplyPatchAction {
            changes: HashMap::from([(
                session_dir.path().join(relative_path),
                ApplyPatchFileChange::Update {
                    unified_diff: r#"@@ -1 +1 @@
-session directory content
+updated session directory content
"#
                    .to_string(),
                    move_path: None,
                    new_content: "updated session directory content\n".to_string(),
                },
            )]),
            patch: argv[1].clone(),
            cwd: session_dir.path().to_path_buf(),
        })
    );
}

#[test]
fn test_apply_patch_resolves_move_path_with_effective_cwd() {
    let session_dir = tempdir().unwrap();
    let worktree_rel = "alt";
    let worktree_dir = session_dir.path().join(worktree_rel);
    fs::create_dir_all(&worktree_dir).unwrap();

    let source_name = "old.txt";
    let dest_name = "renamed.txt";
    let source_path = worktree_dir.join(source_name);
    fs::write(&source_path, "before\n").unwrap();

    let patch = wrap_patch(&format!(
        r#"*** Update File: {source_name}
*** Move to: {dest_name}
@@
-before
+after"#
    ));

    let shell_script = format!("cd {worktree_rel} && apply_patch <<'PATCH'\n{patch}\nPATCH");
    let argv = vec!["bash".into(), "-lc".into(), shell_script];

    let result = maybe_parse_apply_patch_verified(&argv, session_dir.path());
    let action = match result {
        MaybeApplyPatchVerified::Body(action) => action,
        other => panic!("expected verified body, got {other:?}"),
    };

    assert_eq!(action.cwd, worktree_dir);

    let change = action
        .changes()
        .get(&worktree_dir.join(source_name))
        .expect("source file change present");

    match change {
        ApplyPatchFileChange::Update { move_path, .. } => {
            assert_eq!(
                move_path.as_deref(),
                Some(worktree_dir.join(dest_name).as_path())
            );
        }
        other => panic!("expected update change, got {other:?}"),
    }
}

#[test]
fn test_apply_patch_fails_on_write_error() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("readonly.txt");
    fs::write(&path, "before\n").unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&path, perms).unwrap();

    let patch = wrap_patch(&format!(
        "*** Update File: {}\n@@\n-before\n+after\n*** End Patch",
        path.display()
    ));

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let result = apply_patch(&patch, &mut stdout, &mut stderr);
    assert!(result.is_err());
}
