//! Folders a person types for a new session: as absolute paths, and
//! completed with Tab after `/new`.

use std::path::Path;
use std::path::PathBuf;

/// `folder` as an absolute path: `~` is the home directory and relative
/// folders start from `base`.
pub(crate) fn resolve(folder: &str, base: &Path) -> PathBuf {
    let home = || dirs::home_dir().unwrap_or_default();
    let path = match folder.strip_prefix('~') {
        Some("") => home(),
        Some(rest) if rest.starts_with('/') => home().join(rest.trim_start_matches('/')),
        _ => base.join(folder),
    };
    path.components().collect()
}

/// The folder typed after `/new` while it is the only argument.
pub(crate) fn typed_folder(text: &str) -> Option<&str> {
    text.strip_prefix("/new ")
        .filter(|typed| !typed.contains(char::is_whitespace))
}

/// `typed` split into the folder it is in, ending in `/` unless empty, and
/// the start of the name being completed.
fn split(typed: &str) -> (&str, &str) {
    typed
        .rsplit_once('/')
        .map_or(("", typed), |(parent, prefix)| {
            (&typed[..=parent.len()], prefix)
        })
}

/// `text` (a `/new` command) completed against this machine's folders.
pub(crate) fn complete(text: &str, base: &Path) -> Option<(String, Vec<String>)> {
    let typed = typed_folder(text)?;
    let (parent, prefix) = split(typed);
    let directory = if parent.is_empty() {
        base.to_path_buf()
    } else {
        resolve(parent, base)
    };
    let names: Vec<String> = std::fs::read_dir(directory)
        .ok()?
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        // `/new` arguments split on whitespace, so such names cannot be typed.
        .filter(|name| !name.contains(char::is_whitespace))
        .filter(|name| {
            name.starts_with(prefix) && (prefix.starts_with('.') || !name.starts_with('.'))
        })
        .collect();
    completion(parent, names)
}

/// `typed` completed with the host's `find_paths` folders, each the typed
/// folder completed and ending in `/`. When the host may have cut the list
/// short (`complete` is false), a folder it left out could share less of
/// the name, so the text stays as typed and the folders are only listed.
pub(crate) fn complete_from_host(
    typed: &str,
    matches: &[nori_protocol::PathMatch],
    complete: bool,
) -> Option<(String, Vec<String>)> {
    let (parent, _) = split(typed);
    let names: Vec<String> = matches
        .iter()
        .filter_map(|found| found.path.strip_prefix(parent))
        .map(|name| name.trim_end_matches('/').to_owned())
        .collect();
    if !complete && !names.is_empty() {
        let listed = names.iter().map(|name| format!("{name}/")).collect();
        return Some((format!("/new {typed}"), listed));
    }
    completion(parent, names)
}

/// The `/new` text completed as far as `names` (folders in `parent`) agree,
/// and those folders when more than one matches.
fn completion(parent: &str, mut names: Vec<String>) -> Option<(String, Vec<String>)> {
    names.sort();
    let first = names.first()?;
    let shared = names.iter().fold(first.as_str(), |shared, name| {
        let end = shared
            .char_indices()
            .zip(name.chars())
            .find(|((_, a), b)| a != b)
            .map_or(shared.len().min(name.len()), |((at, _), _)| at);
        &shared[..end]
    });
    let completed = if names.len() == 1 {
        format!("/new {parent}{shared}/")
    } else {
        format!("/new {parent}{shared}")
    };
    let candidates = if names.len() > 1 {
        names.iter().map(|name| format!("{name}/")).collect()
    } else {
        Vec::new()
    };
    Some((completed, candidates))
}
