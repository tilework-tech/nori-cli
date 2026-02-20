use super::*;
use simplify::is_small_formatting_command;
use std::path::PathBuf;
use std::string::ToString;

fn shlex_split_safe(s: &str) -> Vec<String> {
    shlex_split(s).unwrap_or_else(|| s.split_whitespace().map(ToString::to_string).collect())
}

fn vec_str(args: &[&str]) -> Vec<String> {
    args.iter().map(ToString::to_string).collect()
}

fn assert_parsed(args: &[String], expected: Vec<ParsedCommand>) {
    let out = parse_command(args);
    assert_eq!(out, expected);
}

#[test]
fn git_status_is_unknown() {
    assert_parsed(
        &vec_str(&["git", "status"]),
        vec![ParsedCommand::Unknown {
            cmd: "git status".to_string(),
        }],
    );
}

#[test]
fn handles_git_pipe_wc() {
    let inner = "git status | wc -l";
    assert_parsed(
        &vec_str(&["bash", "-lc", inner]),
        vec![ParsedCommand::Unknown {
            cmd: "git status".to_string(),
        }],
    );
}

#[test]
fn bash_lc_redirect_not_quoted() {
    let inner = "echo foo > bar";
    assert_parsed(
        &vec_str(&["bash", "-lc", inner]),
        vec![ParsedCommand::Unknown {
            cmd: "echo foo > bar".to_string(),
        }],
    );
}

#[test]
fn handles_complex_bash_command_head() {
    let inner =
        "rg --version && node -v && pnpm -v && rg --files | wc -l && rg --files | head -n 40";
    assert_parsed(
        &vec_str(&["bash", "-lc", inner]),
        vec![
            // Expect commands in left-to-right execution order
            ParsedCommand::Search {
                cmd: "rg --version".to_string(),
                query: None,
                path: None,
            },
            ParsedCommand::Unknown {
                cmd: "node -v".to_string(),
            },
            ParsedCommand::Unknown {
                cmd: "pnpm -v".to_string(),
            },
            ParsedCommand::Search {
                cmd: "rg --files".to_string(),
                query: None,
                path: None,
            },
            ParsedCommand::Unknown {
                cmd: "head -n 40".to_string(),
            },
        ],
    );
}

#[test]
fn supports_searching_for_navigate_to_route() -> anyhow::Result<()> {
    let inner = "rg -n \"navigate-to-route\" -S";
    assert_parsed(
        &vec_str(&["bash", "-lc", inner]),
        vec![ParsedCommand::Search {
            cmd: "rg -n navigate-to-route -S".to_string(),
            query: Some("navigate-to-route".to_string()),
            path: None,
        }],
    );
    Ok(())
}

#[test]
fn handles_complex_bash_command() {
    let inner = "rg -n \"BUG|FIXME|TODO|XXX|HACK\" -S | head -n 200";
    assert_parsed(
        &vec_str(&["bash", "-lc", inner]),
        vec![
            ParsedCommand::Search {
                cmd: "rg -n 'BUG|FIXME|TODO|XXX|HACK' -S".to_string(),
                query: Some("BUG|FIXME|TODO|XXX|HACK".to_string()),
                path: None,
            },
            ParsedCommand::Unknown {
                cmd: "head -n 200".to_string(),
            },
        ],
    );
}

#[test]
fn supports_rg_files_with_path_and_pipe() {
    let inner = "rg --files webview/src | sed -n";
    assert_parsed(
        &vec_str(&["bash", "-lc", inner]),
        vec![ParsedCommand::Search {
            cmd: "rg --files webview/src".to_string(),
            query: None,
            path: Some("webview".to_string()),
        }],
    );
}

#[test]
fn supports_rg_files_then_head() {
    let inner = "rg --files | head -n 50";
    assert_parsed(
        &vec_str(&["bash", "-lc", inner]),
        vec![
            ParsedCommand::Search {
                cmd: "rg --files".to_string(),
                query: None,
                path: None,
            },
            ParsedCommand::Unknown {
                cmd: "head -n 50".to_string(),
            },
        ],
    );
}

#[test]
fn supports_cat() {
    let inner = "cat webview/README.md";
    assert_parsed(
        &vec_str(&["bash", "-lc", inner]),
        vec![ParsedCommand::Read {
            cmd: inner.to_string(),
            name: "README.md".to_string(),
            path: PathBuf::from("webview/README.md"),
        }],
    );
}

#[test]
fn zsh_lc_supports_cat() {
    let inner = "cat README.md";
    assert_parsed(
        &vec_str(&["zsh", "-lc", inner]),
        vec![ParsedCommand::Read {
            cmd: inner.to_string(),
            name: "README.md".to_string(),
            path: PathBuf::from("README.md"),
        }],
    );
}

#[test]
fn cd_then_cat_is_single_read() {
    assert_parsed(
        &shlex_split_safe("cd foo && cat foo.txt"),
        vec![ParsedCommand::Read {
            cmd: "cat foo.txt".to_string(),
            name: "foo.txt".to_string(),
            path: PathBuf::from("foo/foo.txt"),
        }],
    );
}

#[test]
fn bash_cd_then_bar_is_same_as_bar() {
    // Ensure a leading `cd` inside bash -lc is dropped when followed by another command.
    assert_parsed(
        &shlex_split_safe("bash -lc 'cd foo && bar'"),
        vec![ParsedCommand::Unknown {
            cmd: "bar".to_string(),
        }],
    );
}

#[test]
fn bash_cd_then_cat_is_read() {
    assert_parsed(
        &shlex_split_safe("bash -lc 'cd foo && cat foo.txt'"),
        vec![ParsedCommand::Read {
            cmd: "cat foo.txt".to_string(),
            name: "foo.txt".to_string(),
            path: PathBuf::from("foo/foo.txt"),
        }],
    );
}

#[test]
fn supports_ls_with_pipe() {
    let inner = "ls -la | sed -n '1,120p'";
    assert_parsed(
        &vec_str(&["bash", "-lc", inner]),
        vec![ParsedCommand::ListFiles {
            cmd: "ls -la".to_string(),
            path: None,
        }],
    );
}

#[test]
fn supports_head_n() {
    let inner = "head -n 50 Cargo.toml";
    assert_parsed(
        &vec_str(&["bash", "-lc", inner]),
        vec![ParsedCommand::Read {
            cmd: inner.to_string(),
            name: "Cargo.toml".to_string(),
            path: PathBuf::from("Cargo.toml"),
        }],
    );
}

#[test]
fn supports_cat_sed_n() {
    let inner = "cat tui/Cargo.toml | sed -n '1,200p'";
    assert_parsed(
        &vec_str(&["bash", "-lc", inner]),
        vec![ParsedCommand::Read {
            cmd: inner.to_string(),
            name: "Cargo.toml".to_string(),
            path: PathBuf::from("tui/Cargo.toml"),
        }],
    );
}

#[test]
fn supports_tail_n_plus() {
    let inner = "tail -n +522 README.md";
    assert_parsed(
        &vec_str(&["bash", "-lc", inner]),
        vec![ParsedCommand::Read {
            cmd: inner.to_string(),
            name: "README.md".to_string(),
            path: PathBuf::from("README.md"),
        }],
    );
}

#[test]
fn supports_tail_n_last_lines() {
    let inner = "tail -n 30 README.md";
    let out = parse_command(&vec_str(&["bash", "-lc", inner]));
    assert_eq!(
        out,
        vec![ParsedCommand::Read {
            cmd: inner.to_string(),
            name: "README.md".to_string(),
            path: PathBuf::from("README.md"),
        }]
    );
}

#[test]
fn supports_npm_run_build_is_unknown() {
    assert_parsed(
        &vec_str(&["npm", "run", "build"]),
        vec![ParsedCommand::Unknown {
            cmd: "npm run build".to_string(),
        }],
    );
}

#[test]
fn supports_grep_recursive_current_dir() {
    assert_parsed(
        &vec_str(&["grep", "-R", "CODEX_SANDBOX_ENV_VAR", "-n", "."]),
        vec![ParsedCommand::Search {
            cmd: "grep -R CODEX_SANDBOX_ENV_VAR -n .".to_string(),
            query: Some("CODEX_SANDBOX_ENV_VAR".to_string()),
            path: Some(".".to_string()),
        }],
    );
}

#[test]
fn supports_grep_recursive_specific_file() {
    assert_parsed(
        &vec_str(&[
            "grep",
            "-R",
            "CODEX_SANDBOX_ENV_VAR",
            "-n",
            "core/src/spawn.rs",
        ]),
        vec![ParsedCommand::Search {
            cmd: "grep -R CODEX_SANDBOX_ENV_VAR -n core/src/spawn.rs".to_string(),
            query: Some("CODEX_SANDBOX_ENV_VAR".to_string()),
            path: Some("spawn.rs".to_string()),
        }],
    );
}

#[test]
fn supports_grep_query_with_slashes_not_shortened() {
    // Query strings may contain slashes and should not be shortened to the basename.
    // Previously, grep queries were passed through short_display_path, which is incorrect.
    assert_parsed(
        &shlex_split_safe("grep -R src/main.rs -n ."),
        vec![ParsedCommand::Search {
            cmd: "grep -R src/main.rs -n .".to_string(),
            query: Some("src/main.rs".to_string()),
            path: Some(".".to_string()),
        }],
    );
}

#[test]
fn supports_grep_weird_backtick_in_query() {
    assert_parsed(
        &shlex_split_safe("grep -R COD`EX_SANDBOX -n"),
        vec![ParsedCommand::Search {
            cmd: "grep -R 'COD`EX_SANDBOX' -n".to_string(),
            query: Some("COD`EX_SANDBOX".to_string()),
            path: None,
        }],
    );
}

#[test]
fn supports_cd_and_rg_files() {
    assert_parsed(
        &shlex_split_safe("cd codex-rs && rg --files"),
        vec![ParsedCommand::Search {
            cmd: "rg --files".to_string(),
            query: None,
            path: None,
        }],
    );
}

// ---- is_small_formatting_command unit tests ----
#[test]
fn small_formatting_always_true_commands() {
    for cmd in [
        "wc", "tr", "cut", "sort", "uniq", "xargs", "tee", "column", "awk",
    ] {
        assert!(is_small_formatting_command(&shlex_split_safe(cmd)));
        assert!(is_small_formatting_command(&shlex_split_safe(&format!(
            "{cmd} -x"
        ))));
    }
}

#[test]
fn head_behavior() {
    // No args -> small formatting
    assert!(is_small_formatting_command(&vec_str(&["head"])));
    // Numeric count only -> not considered small formatting by implementation
    assert!(!is_small_formatting_command(&shlex_split_safe(
        "head -n 40"
    )));
    // With explicit file -> not small formatting
    assert!(!is_small_formatting_command(&shlex_split_safe(
        "head -n 40 file.txt"
    )));
    // File only (no count) -> treated as small formatting by implementation
    assert!(is_small_formatting_command(&vec_str(&["head", "file.txt"])));
}

#[test]
fn tail_behavior() {
    // No args -> small formatting
    assert!(is_small_formatting_command(&vec_str(&["tail"])));
    // Numeric with plus offset -> not small formatting
    assert!(!is_small_formatting_command(&shlex_split_safe(
        "tail -n +10"
    )));
    assert!(!is_small_formatting_command(&shlex_split_safe(
        "tail -n +10 file.txt"
    )));
    // Numeric count
    assert!(!is_small_formatting_command(&shlex_split_safe(
        "tail -n 30"
    )));
    assert!(!is_small_formatting_command(&shlex_split_safe(
        "tail -n 30 file.txt"
    )));
    // File only -> small formatting by implementation
    assert!(is_small_formatting_command(&vec_str(&["tail", "file.txt"])));
}

#[test]
fn sed_behavior() {
    // Plain sed -> small formatting
    assert!(is_small_formatting_command(&vec_str(&["sed"])));
    // sed -n <range> (no file) -> still small formatting
    assert!(is_small_formatting_command(&vec_str(&["sed", "-n", "10p"])));
    // Valid range with file -> not small formatting
    assert!(!is_small_formatting_command(&shlex_split_safe(
        "sed -n 10p file.txt"
    )));
    assert!(!is_small_formatting_command(&shlex_split_safe(
        "sed -n 1,200p file.txt"
    )));
    // Invalid ranges with file -> small formatting
    assert!(is_small_formatting_command(&shlex_split_safe(
        "sed -n p file.txt"
    )));
    assert!(is_small_formatting_command(&shlex_split_safe(
        "sed -n +10p file.txt"
    )));
}

#[test]
fn empty_tokens_is_not_small() {
    let empty: Vec<String> = Vec::new();
    assert!(!is_small_formatting_command(&empty));
}

#[test]
fn supports_nl_then_sed_reading() {
    let inner = "nl -ba core/src/parse_command.rs | sed -n '1200,1720p'";
    assert_parsed(
        &vec_str(&["bash", "-lc", inner]),
        vec![ParsedCommand::Read {
            cmd: inner.to_string(),
            name: "parse_command.rs".to_string(),
            path: PathBuf::from("core/src/parse_command.rs"),
        }],
    );
}

#[test]
fn supports_sed_n() {
    let inner = "sed -n '2000,2200p' tui/src/history_cell.rs";
    assert_parsed(
        &vec_str(&["bash", "-lc", inner]),
        vec![ParsedCommand::Read {
            cmd: inner.to_string(),
            name: "history_cell.rs".to_string(),
            path: PathBuf::from("tui/src/history_cell.rs"),
        }],
    );
}

#[test]
fn filters_out_printf() {
    let inner = r#"printf "\n===== ansi-escape/Cargo.toml =====\n"; cat -- ansi-escape/Cargo.toml"#;
    assert_parsed(
        &vec_str(&["bash", "-lc", inner]),
        vec![ParsedCommand::Read {
            cmd: "cat -- ansi-escape/Cargo.toml".to_string(),
            name: "Cargo.toml".to_string(),
            path: PathBuf::from("ansi-escape/Cargo.toml"),
        }],
    );
}

#[test]
fn drops_yes_in_pipelines() {
    // Inside bash -lc, `yes | rg --files` should focus on the primary command.
    let inner = "yes | rg --files";
    assert_parsed(
        &vec_str(&["bash", "-lc", inner]),
        vec![ParsedCommand::Search {
            cmd: "rg --files".to_string(),
            query: None,
            path: None,
        }],
    );
}

#[test]
fn supports_sed_n_then_nl_as_search() {
    // Ensure `sed -n '<range>' <file> | nl -ba` is summarized as a search for that file.
    let args = shlex_split_safe(
        "sed -n '260,640p' exec/src/event_processor_with_human_output.rs | nl -ba",
    );
    assert_parsed(
        &args,
        vec![ParsedCommand::Read {
            cmd: "sed -n '260,640p' exec/src/event_processor_with_human_output.rs".to_string(),
            name: "event_processor_with_human_output.rs".to_string(),
            path: PathBuf::from("exec/src/event_processor_with_human_output.rs"),
        }],
    );
}

#[test]
fn preserves_rg_with_spaces() {
    assert_parsed(
        &shlex_split_safe("yes | rg -n 'foo bar' -S"),
        vec![ParsedCommand::Search {
            cmd: "rg -n 'foo bar' -S".to_string(),
            query: Some("foo bar".to_string()),
            path: None,
        }],
    );
}

#[test]
fn ls_with_glob() {
    assert_parsed(
        &shlex_split_safe("ls -I '*.test.js'"),
        vec![ParsedCommand::ListFiles {
            cmd: "ls -I '*.test.js'".to_string(),
            path: None,
        }],
    );
}

#[test]
fn trim_on_semicolon() {
    assert_parsed(
        &shlex_split_safe("rg foo ; echo done"),
        vec![
            ParsedCommand::Search {
                cmd: "rg foo".to_string(),
                query: Some("foo".to_string()),
                path: None,
            },
            ParsedCommand::Unknown {
                cmd: "echo done".to_string(),
            },
        ],
    );
}

#[test]
fn split_on_or_connector() {
    // Ensure we split commands on the logical OR operator as well.
    assert_parsed(
        &shlex_split_safe("rg foo || echo done"),
        vec![
            ParsedCommand::Search {
                cmd: "rg foo".to_string(),
                query: Some("foo".to_string()),
                path: None,
            },
            ParsedCommand::Unknown {
                cmd: "echo done".to_string(),
            },
        ],
    );
}

#[test]
fn parses_mixed_sequence_with_pipes_semicolons_and_or() {
    // Provided long command sequence combining sequencing, pipelines, and ORs.
    let inner = "pwd; ls -la; rg --files -g '!target' | wc -l; rg -n '^\\[workspace\\]' -n Cargo.toml || true; rg -n '^\\[package\\]' -n */Cargo.toml || true; cargo --version; rustc --version; cargo clippy --workspace --all-targets --all-features -q";
    let args = vec_str(&["bash", "-lc", inner]);

    let expected = vec![
        ParsedCommand::Unknown {
            cmd: "pwd".to_string(),
        },
        ParsedCommand::ListFiles {
            cmd: shlex_join(&shlex_split_safe("ls -la")),
            path: None,
        },
        ParsedCommand::Search {
            cmd: shlex_join(&shlex_split_safe("rg --files -g '!target'")),
            query: None,
            path: Some("!target".to_string()),
        },
        ParsedCommand::Search {
            cmd: shlex_join(&shlex_split_safe("rg -n '^\\[workspace\\]' -n Cargo.toml")),
            query: Some("^\\[workspace\\]".to_string()),
            path: Some("Cargo.toml".to_string()),
        },
        ParsedCommand::Search {
            cmd: shlex_join(&shlex_split_safe("rg -n '^\\[package\\]' -n */Cargo.toml")),
            query: Some("^\\[package\\]".to_string()),
            path: Some("Cargo.toml".to_string()),
        },
        ParsedCommand::Unknown {
            cmd: shlex_join(&shlex_split_safe("cargo --version")),
        },
        ParsedCommand::Unknown {
            cmd: shlex_join(&shlex_split_safe("rustc --version")),
        },
        ParsedCommand::Unknown {
            cmd: shlex_join(&shlex_split_safe(
                "cargo clippy --workspace --all-targets --all-features -q",
            )),
        },
    ];

    assert_parsed(&args, expected);
}

#[test]
fn strips_true_in_sequence() {
    // `true` should be dropped from parsed sequences
    assert_parsed(
        &shlex_split_safe("true && rg --files"),
        vec![ParsedCommand::Search {
            cmd: "rg --files".to_string(),
            query: None,
            path: None,
        }],
    );

    assert_parsed(
        &shlex_split_safe("rg --files && true"),
        vec![ParsedCommand::Search {
            cmd: "rg --files".to_string(),
            query: None,
            path: None,
        }],
    );
}

#[test]
fn strips_true_inside_bash_lc() {
    let inner = "true && rg --files";
    assert_parsed(
        &vec_str(&["bash", "-lc", inner]),
        vec![ParsedCommand::Search {
            cmd: "rg --files".to_string(),
            query: None,
            path: None,
        }],
    );

    let inner2 = "rg --files || true";
    assert_parsed(
        &vec_str(&["bash", "-lc", inner2]),
        vec![ParsedCommand::Search {
            cmd: "rg --files".to_string(),
            query: None,
            path: None,
        }],
    );
}

#[test]
fn shorten_path_on_windows() {
    assert_parsed(
        &shlex_split_safe(r#"cat "pkg\src\main.rs""#),
        vec![ParsedCommand::Read {
            cmd: r#"cat "pkg\\src\\main.rs""#.to_string(),
            name: "main.rs".to_string(),
            path: PathBuf::from(r#"pkg\src\main.rs"#),
        }],
    );
}

#[test]
fn head_with_no_space() {
    assert_parsed(
        &shlex_split_safe("bash -lc 'head -n50 Cargo.toml'"),
        vec![ParsedCommand::Read {
            cmd: "head -n50 Cargo.toml".to_string(),
            name: "Cargo.toml".to_string(),
            path: PathBuf::from("Cargo.toml"),
        }],
    );
}

#[test]
fn bash_dash_c_pipeline_parsing() {
    // Ensure -c is handled similarly to -lc by normalization
    let inner = "rg --files | head -n 1";
    assert_parsed(
        &shlex_split_safe(inner),
        vec![
            ParsedCommand::Search {
                cmd: "rg --files".to_string(),
                query: None,
                path: None,
            },
            ParsedCommand::Unknown {
                cmd: "head -n 1".to_string(),
            },
        ],
    );
}

#[test]
fn tail_with_no_space() {
    assert_parsed(
        &shlex_split_safe("bash -lc 'tail -n+10 README.md'"),
        vec![ParsedCommand::Read {
            cmd: "tail -n+10 README.md".to_string(),
            name: "README.md".to_string(),
            path: PathBuf::from("README.md"),
        }],
    );
}

#[test]
fn grep_with_query_and_path() {
    assert_parsed(
        &shlex_split_safe("grep -R TODO src"),
        vec![ParsedCommand::Search {
            cmd: "grep -R TODO src".to_string(),
            query: Some("TODO".to_string()),
            path: Some("src".to_string()),
        }],
    );
}

#[test]
fn rg_with_equals_style_flags() {
    assert_parsed(
        &shlex_split_safe("rg --colors=never -n foo src"),
        vec![ParsedCommand::Search {
            cmd: "rg '--colors=never' -n foo src".to_string(),
            query: Some("foo".to_string()),
            path: Some("src".to_string()),
        }],
    );
}

#[test]
fn cat_with_double_dash_and_sed_ranges() {
    // cat -- <file> should be treated as a read of that file
    assert_parsed(
        &shlex_split_safe("cat -- ./-strange-file-name"),
        vec![ParsedCommand::Read {
            cmd: "cat -- ./-strange-file-name".to_string(),
            name: "-strange-file-name".to_string(),
            path: PathBuf::from("./-strange-file-name"),
        }],
    );

    // sed -n <range> <file> should be treated as a read of <file>
    assert_parsed(
        &shlex_split_safe("sed -n '12,20p' Cargo.toml"),
        vec![ParsedCommand::Read {
            cmd: "sed -n '12,20p' Cargo.toml".to_string(),
            name: "Cargo.toml".to_string(),
            path: PathBuf::from("Cargo.toml"),
        }],
    );
}

#[test]
fn drop_trailing_nl_in_pipeline() {
    // When an `nl` stage has only flags, it should be dropped from the summary
    assert_parsed(
        &shlex_split_safe("rg --files | nl -ba"),
        vec![ParsedCommand::Search {
            cmd: "rg --files".to_string(),
            query: None,
            path: None,
        }],
    );
}

#[test]
fn ls_with_time_style_and_path() {
    assert_parsed(
        &shlex_split_safe("ls --time-style=long-iso ./dist"),
        vec![ParsedCommand::ListFiles {
            cmd: "ls '--time-style=long-iso' ./dist".to_string(),
            // short_display_path drops "dist" and shows "." as the last useful segment
            path: Some(".".to_string()),
        }],
    );
}

#[test]
fn fd_file_finder_variants() {
    assert_parsed(
        &shlex_split_safe("fd -t f src/"),
        vec![ParsedCommand::Search {
            cmd: "fd -t f src/".to_string(),
            query: None,
            path: Some("src".to_string()),
        }],
    );

    // fd with query and path should capture both
    assert_parsed(
        &shlex_split_safe("fd main src"),
        vec![ParsedCommand::Search {
            cmd: "fd main src".to_string(),
            query: Some("main".to_string()),
            path: Some("src".to_string()),
        }],
    );
}

#[test]
fn find_basic_name_filter() {
    assert_parsed(
        &shlex_split_safe("find . -name '*.rs'"),
        vec![ParsedCommand::Search {
            cmd: "find . -name '*.rs'".to_string(),
            query: Some("*.rs".to_string()),
            path: Some(".".to_string()),
        }],
    );
}

#[test]
fn find_type_only_path() {
    assert_parsed(
        &shlex_split_safe("find src -type f"),
        vec![ParsedCommand::Search {
            cmd: "find src -type f".to_string(),
            query: None,
            path: Some("src".to_string()),
        }],
    );
}

#[test]
fn bin_bash_lc_sed() {
    assert_parsed(
        &shlex_split_safe("/bin/bash -lc 'sed -n '1,10p' Cargo.toml'"),
        vec![ParsedCommand::Read {
            cmd: "sed -n '1,10p' Cargo.toml".to_string(),
            name: "Cargo.toml".to_string(),
            path: PathBuf::from("Cargo.toml"),
        }],
    );
}
#[test]
fn bin_zsh_lc_sed() {
    assert_parsed(
        &shlex_split_safe("/bin/zsh -lc 'sed -n '1,10p' Cargo.toml'"),
        vec![ParsedCommand::Read {
            cmd: "sed -n '1,10p' Cargo.toml".to_string(),
            name: "Cargo.toml".to_string(),
            path: PathBuf::from("Cargo.toml"),
        }],
    );
}

#[test]
fn powershell_command_is_stripped() {
    assert_parsed(
        &vec_str(&["powershell", "-Command", "Get-ChildItem"]),
        vec![ParsedCommand::Unknown {
            cmd: "Get-ChildItem".to_string(),
        }],
    );
}

#[test]
fn pwsh_with_noprofile_and_c_alias_is_stripped() {
    assert_parsed(
        &vec_str(&["pwsh", "-NoProfile", "-c", "Write-Host hi"]),
        vec![ParsedCommand::Unknown {
            cmd: "Write-Host hi".to_string(),
        }],
    );
}

#[test]
fn powershell_with_path_is_stripped() {
    let command = if cfg!(windows) {
        "C:\\windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"
    } else {
        "/usr/local/bin/powershell.exe"
    };

    assert_parsed(
        &vec_str(&[command, "-NoProfile", "-c", "Write-Host hi"]),
        vec![ParsedCommand::Unknown {
            cmd: "Write-Host hi".to_string(),
        }],
    );
}
