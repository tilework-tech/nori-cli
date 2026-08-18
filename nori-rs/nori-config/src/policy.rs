use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

use serde::Deserialize;
use serde::Serialize;

/// Determines when the user is consulted before an agent operation proceeds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AskForApproval {
    #[serde(rename = "untrusted")]
    UnlessTrusted,
    OnFailure,
    #[default]
    OnRequest,
    Never,
}

/// Resolved execution restrictions for agent shell commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum SandboxPolicy {
    #[serde(rename = "danger-full-access")]
    DangerFullAccess,
    #[serde(rename = "read-only")]
    ReadOnly,
    #[serde(rename = "workspace-write")]
    WorkspaceWrite {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        writable_roots: Vec<PathBuf>,
        #[serde(default)]
        network_access: bool,
        #[serde(default)]
        exclude_tmpdir_env_var: bool,
        #[serde(default)]
        exclude_slash_tmp: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WritableRoot {
    pub root: PathBuf,
    pub read_only_subpaths: Vec<PathBuf>,
}

impl WritableRoot {
    pub fn is_path_writable(&self, path: &Path) -> bool {
        path.starts_with(&self.root)
            && !self
                .read_only_subpaths
                .iter()
                .any(|subpath| path.starts_with(subpath))
    }
}

impl SandboxPolicy {
    /// Serializes the policy for the private Linux sandbox helper boundary.
    pub fn to_helper_arg(&self) -> Result<String, toml::ser::Error> {
        toml::to_string(self)
    }

    pub fn new_read_only_policy() -> Self {
        Self::ReadOnly
    }

    pub fn new_workspace_write_policy() -> Self {
        Self::WorkspaceWrite {
            writable_roots: Vec::new(),
            network_access: false,
            exclude_tmpdir_env_var: false,
            exclude_slash_tmp: false,
        }
    }

    pub fn has_full_disk_read_access(&self) -> bool {
        true
    }

    pub fn has_full_disk_write_access(&self) -> bool {
        matches!(self, Self::DangerFullAccess)
    }

    pub fn has_full_network_access(&self) -> bool {
        match self {
            Self::DangerFullAccess => true,
            Self::ReadOnly => false,
            Self::WorkspaceWrite { network_access, .. } => *network_access,
        }
    }

    pub fn get_writable_roots_with_cwd(&self, cwd: &Path) -> Vec<WritableRoot> {
        let Self::WorkspaceWrite {
            writable_roots,
            exclude_tmpdir_env_var,
            exclude_slash_tmp,
            ..
        } = self
        else {
            return Vec::new();
        };

        let mut roots = writable_roots.clone();
        roots.push(cwd.to_path_buf());
        if cfg!(unix) && !exclude_slash_tmp {
            let slash_tmp = PathBuf::from("/tmp");
            if slash_tmp.is_dir() {
                roots.push(slash_tmp);
            }
        }
        if !exclude_tmpdir_env_var
            && let Some(tmpdir) = std::env::var_os("TMPDIR")
            && !tmpdir.is_empty()
        {
            roots.push(PathBuf::from(tmpdir));
        }

        roots
            .into_iter()
            .map(|root| {
                let git_dir = root.join(".git");
                let read_only_subpaths = git_dir.is_dir().then_some(git_dir).into_iter().collect();
                WritableRoot {
                    root,
                    read_only_subpaths,
                }
            })
            .collect()
    }
}

impl FromStr for SandboxPolicy {
    type Err = toml::de::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        toml::from_str(value)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::SandboxPolicy;

    #[test]
    fn sandbox_policy_helper_argument_round_trips() {
        let policy = SandboxPolicy::WorkspaceWrite {
            writable_roots: vec!["/workspace with spaces".into()],
            network_access: true,
            exclude_tmpdir_env_var: true,
            exclude_slash_tmp: false,
        };

        let encoded = policy
            .to_helper_arg()
            .expect("sandbox policy should serialize");

        assert_eq!(
            SandboxPolicy::from_str(&encoded).expect("sandbox policy should deserialize"),
            policy
        );
    }
}
