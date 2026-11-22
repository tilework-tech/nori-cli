//! Compose an ACP stdio server and its optional outer launcher as literal argv.
use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::Path;
use tokio::process::Command;

mod owned_runtime;
pub use owned_runtime::OwnedRuntime;
pub(crate) use owned_runtime::private_dir;
pub(crate) use owned_runtime::private_file;

/// Local servers inherit the client environment. Managed servers receive only
/// variables supplied by the admitted launch and its binding.
pub enum ChildEnvironment {
    Inherit,
    Empty,
}

/// The executable and arguments passed to the process supervisor.
pub struct ServerCommand {
    program: OsString,
    args: Vec<OsString>,
}

impl ServerCommand {
    pub fn new(program: impl AsRef<OsStr>, args: &[String]) -> Self {
        Self {
            program: program.as_ref().to_owned(),
            args: args.iter().map(OsString::from).collect(),
        }
    }

    /// Place the entire server invocation after the wrapper's own arguments.
    pub fn with_wrapper(self, program: impl AsRef<OsStr>, args: &[String]) -> Self {
        let mut wrapper = Self::new(program, args);
        wrapper.args.push(self.program);
        wrapper.args.extend(self.args);
        wrapper
    }

    /// Complete invocation, suitable for passing through an outer OS sandbox.
    pub fn argv(&self) -> impl Iterator<Item = &OsStr> {
        std::iter::once(self.program.as_os_str()).chain(self.args.iter().map(OsString::as_os_str))
    }

    /// Apply launch settings to this child without changing the host environment.
    pub fn command<'a>(
        &self,
        base: ChildEnvironment,
        settings: impl IntoIterator<Item = (&'a String, &'a String)>,
        remove: &[String],
        runtime_dir: Option<&Path>,
    ) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        if matches!(base, ChildEnvironment::Empty) {
            command.env_clear();
        }
        for key in remove {
            command.env_remove(key);
        }
        command.envs(settings);
        if let Some(dir) = runtime_dir {
            command.env(nori_config::AGENT_RUNTIME_ENV, dir);
        }
        command
    }
}
