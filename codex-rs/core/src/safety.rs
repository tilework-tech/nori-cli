use crate::exec::SandboxType;

#[cfg(target_os = "windows")]
use std::sync::atomic::AtomicBool;
#[cfg(target_os = "windows")]
use std::sync::atomic::Ordering;

#[cfg(target_os = "windows")]
static WINDOWS_SANDBOX_ENABLED: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "windows")]
pub fn set_windows_sandbox_enabled(enabled: bool) {
    WINDOWS_SANDBOX_ENABLED.store(enabled, Ordering::Relaxed);
}

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
pub fn set_windows_sandbox_enabled(_enabled: bool) {}

pub fn get_platform_sandbox() -> Option<SandboxType> {
    if cfg!(target_os = "macos") {
        Some(SandboxType::MacosSeatbelt)
    } else if cfg!(target_os = "linux") {
        Some(SandboxType::LinuxSeccomp)
    } else if cfg!(target_os = "windows") {
        #[cfg(target_os = "windows")]
        {
            if WINDOWS_SANDBOX_ENABLED.load(Ordering::Relaxed) {
                return Some(SandboxType::WindowsRestrictedToken);
            }
        }
        None
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::path::Component;
    use std::path::Path;
    use std::path::PathBuf;

    use codex_apply_patch::ApplyPatchAction;
    use codex_apply_patch::ApplyPatchFileChange;
    use codex_protocol::protocol::SandboxPolicy;
    use tempfile::TempDir;

    fn is_write_patch_constrained_to_writable_paths(
        action: &ApplyPatchAction,
        sandbox_policy: &SandboxPolicy,
        cwd: &Path,
    ) -> bool {
        let writable_roots = match sandbox_policy {
            SandboxPolicy::ReadOnly => {
                return false;
            }
            SandboxPolicy::DangerFullAccess => {
                return true;
            }
            SandboxPolicy::WorkspaceWrite { .. } => sandbox_policy.get_writable_roots_with_cwd(cwd),
        };

        fn normalize(path: &Path) -> Option<PathBuf> {
            let mut out = PathBuf::new();
            for comp in path.components() {
                match comp {
                    Component::ParentDir => {
                        out.pop();
                    }
                    Component::CurDir => { /* skip */ }
                    other => out.push(other.as_os_str()),
                }
            }
            Some(out)
        }

        let is_path_writable = |p: &PathBuf| {
            let abs = if p.is_absolute() {
                p.clone()
            } else {
                cwd.join(p)
            };
            let abs = match normalize(&abs) {
                Some(v) => v,
                None => return false,
            };

            writable_roots
                .iter()
                .any(|writable_root| writable_root.is_path_writable(&abs))
        };

        for (path, change) in action.changes() {
            match change {
                ApplyPatchFileChange::Add { .. } | ApplyPatchFileChange::Delete { .. } => {
                    if !is_path_writable(path) {
                        return false;
                    }
                }
                ApplyPatchFileChange::Update { move_path, .. } => {
                    if !is_path_writable(path) {
                        return false;
                    }
                    if let Some(dest) = move_path
                        && !is_path_writable(dest)
                    {
                        return false;
                    }
                }
            }
        }

        true
    }

    #[test]
    fn test_writable_roots_constraint() {
        let tmp = TempDir::new().unwrap();
        let cwd = tmp.path().to_path_buf();
        let parent = cwd.parent().unwrap().to_path_buf();

        let make_add_change = |p: PathBuf| ApplyPatchAction::new_add_for_test(&p, "".to_string());

        let add_inside = make_add_change(cwd.join("inner.txt"));
        let add_outside = make_add_change(parent.join("outside.txt"));

        let policy_workspace_only = SandboxPolicy::WorkspaceWrite {
            writable_roots: vec![],
            network_access: false,
            exclude_tmpdir_env_var: true,
            exclude_slash_tmp: true,
        };

        assert!(is_write_patch_constrained_to_writable_paths(
            &add_inside,
            &policy_workspace_only,
            &cwd,
        ));

        assert!(!is_write_patch_constrained_to_writable_paths(
            &add_outside,
            &policy_workspace_only,
            &cwd,
        ));

        let policy_with_parent = SandboxPolicy::WorkspaceWrite {
            writable_roots: vec![parent],
            network_access: false,
            exclude_tmpdir_env_var: true,
            exclude_slash_tmp: true,
        };
        assert!(is_write_patch_constrained_to_writable_paths(
            &add_outside,
            &policy_with_parent,
            &cwd,
        ));
    }
}
