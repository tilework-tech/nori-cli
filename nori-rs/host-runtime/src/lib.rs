//! The ACP service used by local foreground, stdio and background hosts.

mod host;
pub use host::service::HostService;
pub use host::state;
pub use host::tui_owned::OwnedHost;
pub use host::tui_owned::StopHost;
pub use host::tui_owned::launch;
