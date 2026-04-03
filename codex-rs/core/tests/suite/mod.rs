// Aggregates all integration tests as modules.
use codex_arg0::arg0_dispatch;
use ctor::ctor;
use tempfile::TempDir;

// This code runs before any other tests are run.
// It allows the test binary to behave like codex and dispatch to apply_patch and codex-linux-sandbox
// based on the arg0.
// NOTE: this doesn't work on ARM
#[ctor]
pub static CODEX_ALIASES_TEMP_DIR: TempDir = unsafe {
    #[allow(clippy::unwrap_used)]
    arg0_dispatch().unwrap()
};

mod auth_refresh;
mod exec;
mod live_cli;
mod rollout_list_find;
mod seatbelt;
mod text_encoding_fix;
