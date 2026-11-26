//! `/new [DIR] [AGENT]`: the folder a new host session starts in, and Tab
//! completion of that folder. Folders are named from the session on screen,
//! or with none, from the attaching terminal's directory on a local host.
//! Tab asks the host for the folders when it serves `find_paths`, so a
//! remote host completes from its own disk; an older host completes
//! against this machine's files. A host serving `find_paths` also names
//! `~` from its own home and, with no session on screen, relative folders
//! from its own folder, so those are sent as typed. An older host takes
//! only absolute folders, so they are resolved here with this machine's
//! home.

use std::path::Path;

use serde_json::Value;
use serde_json::json;

const USAGE: &str = "Usage: /new [folder] [agent]";

/// Where `/new` starts and resolves relative folders.
pub(super) enum Base<'a> {
    /// The folder of the session on screen, on the host's disk.
    Session(&'a Path),
    /// The attaching terminal's directory; a remote host does not have it.
    Terminal(&'a Path),
}

/// `_nori/session/new` parameters for the arguments after `/new`. A lone
/// argument names a folder when it looks like a path, or on a local host
/// when it is a folder under the base, and an agent otherwise. Without a
/// folder, a session starts in the session's folder, or for a local host
/// in the terminal's; a remote host otherwise picks its own. `host_names`
/// is whether the host resolves `~` and relative folders itself.
pub(super) fn params(
    args: &str,
    base: Base<'_>,
    folders: bool,
    host_names: bool,
    remote: bool,
) -> Result<Value, String> {
    let on_terminal = matches!(base, Base::Terminal(_));
    let (base, default) = match base {
        Base::Session(folder) => (folder, Some(folder.to_path_buf())),
        Base::Terminal(folder) => (folder, (!remote).then(|| folder.to_path_buf())),
    };
    let host_resolves = |folder: &str| {
        host_names
            && (folder.starts_with('~') || (on_terminal && remote && !folder.starts_with('/')))
    };
    let words: Vec<&str> = args.split_whitespace().collect();
    let (folder, agent) = match words.as_slice() {
        [] => (None, None),
        [one]
            if one.starts_with(['/', '~', '.'])
                || one.contains('/')
                || (!remote && base.join(one).is_dir()) =>
        {
            (Some(*one), None)
        }
        [one] => (None, Some(*one)),
        [folder, agent] => (Some(*folder), Some(*agent)),
        _ => return Err(USAGE.into()),
    };
    let mut params = json!({});
    if let Some(cwd) = folder
        .map(|folder| match host_resolves(folder) {
            true => std::path::PathBuf::from(folder),
            false => crate::session_folder::resolve(folder, base),
        })
        .or(default)
        && folders
    {
        params["cwd"] = json!(cwd.to_str().ok_or("The folder is not valid UTF-8.")?);
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
