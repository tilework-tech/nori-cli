//! Parsing and filtering of Anthropic's published Claude model catalog.
//!
//! Anthropic generates the canonical list of Claude model ids into the Python
//! SDK's `model.py` as a `Literal[...]` union, and publishes model lifecycle
//! state as a markdown table on the deprecations page. Both are public and
//! unauthenticated, so nori reads them instead of maintaining its own list.

use std::collections::HashSet;

/// Extracts Claude model ids from the Anthropic SDK's generated `model.py`.
///
/// Reads the quoted entries of the `Literal[...]` union. Returns an empty list
/// when the source contains no recognizable model ids, so an upstream format
/// change degrades to "no catalog" rather than producing partial garbage.
pub(super) fn parse_model_ids(source: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    source
        .split('"')
        .skip(1)
        .step_by(2)
        .filter(|quoted| is_model_id(quoted))
        .filter(|quoted| seen.insert(*quoted))
        .map(str::to_string)
        .collect()
}

/// Whether `value` looks like a Claude model id. Also guards the on-disk cache,
/// so a hand-edited or foreign cache file cannot inject arbitrary strings into
/// the settings nori hands to Claude Code.
pub(super) fn is_model_id(value: &str) -> bool {
    value.starts_with("claude-")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Filters a catalog down to the models worth offering in the picker.
///
/// Drops models Anthropic no longer lists as `Active`, and drops dated
/// variants (`…-20251101`) whose undated alias is already present, since the
/// two select the same model. Models absent from the deprecations table are
/// kept: a model too new to be listed is not a retired one.
pub(super) fn usable_models(ids: Vec<String>, deprecations_markdown: &str) -> Vec<String> {
    let retired = retired_ids(deprecations_markdown);
    let listed = ids
        .into_iter()
        .filter(|id| !retired.contains(id))
        .collect::<Vec<_>>();

    let aliases = listed.iter().cloned().collect::<HashSet<_>>();
    listed
        .iter()
        .filter(|id| match id.rsplit_once('-') {
            Some((prefix, date))
                if date.len() == 8 && date.bytes().all(|byte| byte.is_ascii_digit()) =>
            {
                !aliases.contains(prefix)
            }
            Some(_) | None => true,
        })
        .cloned()
        .collect()
}

/// Reads the "Model status" table, collecting every model whose current state
/// is something other than `Active`.
///
/// The scan is anchored to that table's header. The page carries several other
/// tables whose first column is a model id, and treating one of those as a
/// status row would silently drop a live model from the picker.
fn retired_ids(deprecations_markdown: &str) -> HashSet<String> {
    let mut retired = HashSet::new();
    let mut in_status_table = false;

    for line in deprecations_markdown.lines() {
        if line.contains("Current state") {
            in_status_table = true;
            continue;
        }
        if !in_status_table {
            continue;
        }
        if !line.trim_start().starts_with('|') {
            in_status_table = false;
            continue;
        }

        let cells = line
            .split('|')
            .map(|cell| cell.trim().trim_matches('`'))
            .collect::<Vec<_>>();
        let (Some(id), Some(state)) = (cells.get(1), cells.get(2)) else {
            continue;
        };
        if id.starts_with("claude-") && !state.is_empty() && *state != "Active" {
            retired.insert((*id).to_string());
        }
    }

    retired
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn parses_model_ids_from_sdk_literal_union() {
        let source = r#"
# File generated from our OpenAPI spec by Stainless.
from typing_extensions import Literal, TypeAlias

Model: TypeAlias = Union[
    Literal[
        "claude-sonnet-5",
        "claude-opus-4-8",
        "claude-haiku-4-5-20251001",
    ],
    str,
]
"#;

        assert_eq!(
            parse_model_ids(source),
            vec![
                "claude-sonnet-5".to_string(),
                "claude-opus-4-8".to_string(),
                "claude-haiku-4-5-20251001".to_string(),
            ]
        );
    }

    #[test]
    fn lists_each_model_once_even_if_the_source_repeats_it() {
        let source = r#"
    Literal[
        "claude-opus-4-8",
        "claude-sonnet-5",
        "claude-opus-4-8",
    ],
"#;

        assert_eq!(
            parse_model_ids(source),
            vec!["claude-opus-4-8".to_string(), "claude-sonnet-5".to_string()],
            "a repeated id must not become a duplicate row in the picker"
        );
    }

    #[test]
    fn returns_no_models_when_source_is_not_a_model_catalog() {
        assert_eq!(parse_model_ids("404: Not Found"), Vec::<String>::new());
        assert_eq!(parse_model_ids(""), Vec::<String>::new());
    }

    /// Mirrors the real page: a status table, then history tables that also
    /// mention model ids.
    const DEPRECATIONS: &str = "\
## Model status

| API model name           | Current state | Deprecated   | Tentative retirement date    |
| ------------------------ | ------------- | ------------ | ---------------------------- |
| claude-opus-4-8          | Active        | N/A          | Not sooner than May 28, 2027 |
| claude-opus-4-1-20250805 | Retired       | June 5, 2026 | August 5, 2026               |

## Deprecation history

### 2026-06-05: Claude Opus 4.1 model

| Retirement date | Deprecated model           | Recommended replacement |
| --------------- | -------------------------- | ----------------------- |
| August 5, 2026  | `claude-opus-4-1-20250805` | `claude-opus-4-8`       |
";

    #[test]
    fn drops_models_no_longer_listed_as_active() {
        let ids = vec![
            "claude-opus-4-8".to_string(),
            "claude-opus-4-1-20250805".to_string(),
            "claude-opus-9-9".to_string(),
        ];

        assert_eq!(
            usable_models(ids, DEPRECATIONS),
            vec!["claude-opus-4-8".to_string(), "claude-opus-9-9".to_string()],
            "a model missing from the table is too new to be listed, not retired"
        );
    }

    #[test]
    fn keeps_models_that_other_tables_merely_mention() {
        let ids = vec!["claude-opus-4-8".to_string()];

        assert_eq!(
            usable_models(ids, DEPRECATIONS),
            vec!["claude-opus-4-8".to_string()],
            "an id named as a replacement elsewhere on the page is still active"
        );
    }

    #[test]
    fn collapses_dated_variants_that_duplicate_an_undated_alias() {
        let ids = vec![
            "claude-opus-4-5".to_string(),
            "claude-opus-4-5-20251101".to_string(),
            "claude-sonnet-4-5-20250929".to_string(),
        ];

        assert_eq!(
            usable_models(ids, ""),
            vec![
                "claude-opus-4-5".to_string(),
                "claude-sonnet-4-5-20250929".to_string(),
            ],
            "a dated id is kept when no undated alias covers it"
        );
    }
}
