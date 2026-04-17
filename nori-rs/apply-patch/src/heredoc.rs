use std::str::Utf8Error;
use std::sync::LazyLock;

use tree_sitter::LanguageError;
use tree_sitter::Parser;
use tree_sitter::Query;
use tree_sitter::QueryCursor;
use tree_sitter::StreamingIterator;
use tree_sitter_bash::LANGUAGE as BASH;

/// Extract the heredoc body (and optional `cd` workdir) from a `bash -lc` script
/// that invokes the apply_patch tool using a heredoc.
///
/// Supported top-level forms (must be the only top-level statement):
/// - `apply_patch <<'EOF'\n...\nEOF`
/// - `cd <path> && apply_patch <<'EOF'\n...\nEOF`
///
/// Notes about matching:
/// - Parsed with Tree-sitter Bash and a strict query that uses anchors so the
///   heredoc-redirected statement is the only top-level statement.
/// - The connector between `cd` and `apply_patch` must be `&&` (not `|` or `||`).
/// - Exactly one positional `word` argument is allowed for `cd` (no flags, no quoted
///   strings, no second argument).
/// - The apply command is validated in-query via `#any-of?` to allow `apply_patch`
///   or `applypatch`.
/// - Preceding or trailing commands (e.g., `echo ...;` or `... && echo done`) do not match.
///
/// Returns `(heredoc_body, Some(path))` when the `cd` variant matches, or
/// `(heredoc_body, None)` for the direct form. Errors are returned if the script
/// cannot be parsed or does not match the allowed patterns.
pub(crate) fn extract_apply_patch_from_bash(
    src: &str,
) -> std::result::Result<(String, Option<String>), ExtractHeredocError> {
    // This function uses a Tree-sitter query to recognize one of two
    // whole-script forms, each expressed as a single top-level statement:
    //
    // 1. apply_patch <<'EOF'\n...\nEOF
    // 2. cd <path> && apply_patch <<'EOF'\n...\nEOF
    //
    // Key ideas when reading the query:
    // - dots (`.`) between named nodes enforces adjacency among named children and
    //   anchor to the start/end of the expression.
    // - we match a single redirected_statement directly under program with leading
    //   and trailing anchors (`.`). This ensures it is the only top-level statement
    //   (so prefixes like `echo ...;` or suffixes like `... && echo done` do not match).
    //
    // Overall, we want to be conservative and only match the intended forms, as other
    // forms are likely to be model errors, or incorrectly interpreted by later code.
    //
    // If you're editing this query, it's helpful to start by creating a debugging binary
    // which will let you see the AST of an arbitrary bash script passed in, and optionally
    // also run an arbitrary query against the AST. This is useful for understanding
    // how tree-sitter parses the script and whether the query syntax is correct. Be sure
    // to test both positive and negative cases.
    static APPLY_PATCH_QUERY: LazyLock<Query> = LazyLock::new(|| {
        let language = BASH.into();
        #[expect(clippy::expect_used)]
        Query::new(
            &language,
            r#"
            (
              program
                . (redirected_statement
                    body: (command
                            name: (command_name (word) @apply_name) .)
                    (#any-of? @apply_name "apply_patch" "applypatch")
                    redirect: (heredoc_redirect
                                . (heredoc_start)
                                . (heredoc_body) @heredoc
                                . (heredoc_end)
                                .))
                .)

            (
              program
                . (redirected_statement
                    body: (list
                            . (command
                                name: (command_name (word) @cd_name) .
                                argument: [
                                  (word) @cd_path
                                  (string (string_content) @cd_path)
                                  (raw_string) @cd_raw_string
                                ] .)
                            "&&"
                            . (command
                                name: (command_name (word) @apply_name))
                            .)
                    (#eq? @cd_name "cd")
                    (#any-of? @apply_name "apply_patch" "applypatch")
                    redirect: (heredoc_redirect
                                . (heredoc_start)
                                . (heredoc_body) @heredoc
                                . (heredoc_end)
                                .))
                .)
            "#,
        )
        .expect("valid bash query")
    });

    let lang = BASH.into();
    let mut parser = Parser::new();
    parser
        .set_language(&lang)
        .map_err(ExtractHeredocError::FailedToLoadBashGrammar)?;
    let tree = parser
        .parse(src, None)
        .ok_or(ExtractHeredocError::FailedToParsePatchIntoAst)?;

    let bytes = src.as_bytes();
    let root = tree.root_node();

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&APPLY_PATCH_QUERY, root, bytes);
    while let Some(m) = matches.next() {
        let mut heredoc_text: Option<String> = None;
        let mut cd_path: Option<String> = None;

        for capture in m.captures.iter() {
            let name = APPLY_PATCH_QUERY.capture_names()[capture.index as usize];
            match name {
                "heredoc" => {
                    let text = capture
                        .node
                        .utf8_text(bytes)
                        .map_err(ExtractHeredocError::HeredocNotUtf8)?
                        .trim_end_matches('\n')
                        .to_string();
                    heredoc_text = Some(text);
                }
                "cd_path" => {
                    let text = capture
                        .node
                        .utf8_text(bytes)
                        .map_err(ExtractHeredocError::HeredocNotUtf8)?
                        .to_string();
                    cd_path = Some(text);
                }
                "cd_raw_string" => {
                    let raw = capture
                        .node
                        .utf8_text(bytes)
                        .map_err(ExtractHeredocError::HeredocNotUtf8)?;
                    let trimmed = raw
                        .strip_prefix('\'')
                        .and_then(|s| s.strip_suffix('\''))
                        .unwrap_or(raw);
                    cd_path = Some(trimmed.to_string());
                }
                _ => {}
            }
        }

        if let Some(heredoc) = heredoc_text {
            return Ok((heredoc, cd_path));
        }
    }

    Err(ExtractHeredocError::CommandDidNotStartWithApplyPatch)
}

#[derive(Debug, PartialEq)]
pub enum ExtractHeredocError {
    CommandDidNotStartWithApplyPatch,
    FailedToLoadBashGrammar(LanguageError),
    HeredocNotUtf8(Utf8Error),
    FailedToParsePatchIntoAst,
    FailedToFindHeredocBody,
}
