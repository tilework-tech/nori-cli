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
