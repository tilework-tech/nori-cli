//! `/new [DIR] [AGENT]`: the folder a new host session starts in, and Tab
//! completion of that folder. Folders are named from the attaching
//! terminal's directory and completed against this machine's files.

use std::path::Path;
use std::path::PathBuf;

use serde_json::Value;
use serde_json::json;

const USAGE: &str = "Usage: /new [folder] [agent]";

/// `_nori/session/new` parameters for the arguments after `/new`. A lone
/// argument names a folder when it looks like a path or is a folder under
/// `base`, and an agent otherwise. Without a folder, a session starts in
/// `base` when the host supports `folders`.
pub(super) fn params(args: &str, base: &Path, folders: bool) -> Result<Value, String> {
    let words: Vec<&str> = args.split_whitespace().collect();
    let (folder, agent) = match words.as_slice() {
        [] => (None, None),
        [one]
            if one.starts_with(['/', '~', '.']) || one.contains('/') || base.join(one).is_dir() =>
        {
            (Some(*one), None)
        }
        [one] => (None, Some(*one)),
        [folder, agent] => (Some(*folder), Some(*agent)),
        _ => return Err(USAGE.into()),
    };
    let mut params = json!({});
    if folders {
        params["cwd"] = json!(folder.map_or_else(|| base.to_path_buf(), |f| resolve(f, base)));
    } else if folder.is_some() {
        return Err(
            "This host does not support session folders; restart it with a newer nori.".into(),
        );
    }
    if let Some(agent) = agent {
        params["agent"] = json!(agent);
    }
    Ok(params)
}

/// `text` (a `/new` command) with its folder completed as far as the
/// matching folders agree, and those folders when more than one matches.
pub(super) fn complete(text: &str, base: &Path) -> Option<(String, Vec<String>)> {
    let typed = text.strip_prefix("/new ")?;
    if typed.contains(char::is_whitespace) {
        return None;
    }
    let (parent, prefix) = typed
        .rsplit_once('/')
        .map_or(("", typed), |(parent, prefix)| {
            (&typed[..=parent.len()], prefix)
        });
    let directory = if parent.is_empty() {
        base.to_path_buf()
    } else {
        resolve(parent, base)
    };
    let mut names: Vec<String> = std::fs::read_dir(directory)
        .ok()?
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| {
            name.starts_with(prefix) && (prefix.starts_with('.') || !name.starts_with('.'))
        })
        .collect();
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

/// `folder` as an absolute path: `~` is the home directory and relative
/// folders start from `base`.
fn resolve(folder: &str, base: &Path) -> PathBuf {
    let home = || dirs::home_dir().unwrap_or_default();
    let path = match folder.strip_prefix('~') {
        Some("") => home(),
        Some(rest) if rest.starts_with('/') => home().join(rest.trim_start_matches('/')),
        _ => base.join(folder),
    };
    path.components().collect()
}
