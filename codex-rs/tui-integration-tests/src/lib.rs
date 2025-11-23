use anyhow::Result;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::time::{Duration, Instant};
use vt100::Parser;

#[cfg(unix)]
/// Helper to set a file descriptor to non-blocking mode
fn set_nonblocking(fd: std::os::unix::io::RawFd) -> Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let result = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
    if result < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

pub use keys::Key;
mod keys;

/// PTY session for driving the codex TUI
pub struct TuiSession {
    _master: Box<dyn portable_pty::MasterPty + Send>,
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
    parser: Parser,
    _temp_dir: Option<tempfile::TempDir>,
}

impl Drop for TuiSession {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("\n=== TUI Screen State at Panic ===");
            eprintln!("{}", self.screen_contents());

            if let Some(tmpdir) = &self._temp_dir {
                let log_path = tmpdir.path().join(".codex-acp.log");
                let log_tail = if let Ok(content) = std::fs::read_to_string(log_path) {
                    let lines: Vec<&str> = content.lines().collect();
                    let start = lines.len().saturating_sub(150);
                    lines[start..].join("\n")
                } else {
                    "<failed to read log file>".to_string()
                };
                eprintln!("\n=== ACP Tracing Subscriber    ===");
                eprintln!("{log_tail}");
            }

            eprintln!("=================================\n");
        }
    }
}

impl TuiSession {
    /// Spawn codex with mock-acp-agent in a temporary directory
    pub fn spawn(rows: u16, cols: u16) -> Result<Self> {
        let temp_dir = tempfile::tempdir()?;
        let hello_py = temp_dir.path().join("hello.py");
        std::fs::write(&hello_py, "print('Hello, World!')")?;

        let config = SessionConfig {
            cwd: Some(temp_dir.path().to_path_buf()),
            ..Default::default()
        };

        Self::spawn_with_config_and_tempdir(rows, cols, config, Some(temp_dir))
    }

    /// Spawn with custom configuration
    /// Creates a temp directory with hello.py if no cwd is specified in config
    pub fn spawn_with_config(rows: u16, cols: u16, mut config: SessionConfig) -> Result<Self> {
        if config.cwd.is_none() {
            let temp_dir = tempfile::tempdir()?;
            let hello_py = temp_dir.path().join("hello.py");
            std::fs::write(&hello_py, "print('Hello, World!')")?;
            config.cwd = Some(temp_dir.path().to_path_buf());
            Self::spawn_with_config_and_tempdir(rows, cols, config, Some(temp_dir))
        } else {
            Self::spawn_with_config_and_tempdir(rows, cols, config, None)
        }
    }

    /// Internal method to spawn with optional temp directory
    fn spawn_with_config_and_tempdir(
        rows: u16,
        cols: u16,
        config: SessionConfig,
        temp_dir: Option<tempfile::TempDir>,
    ) -> Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new(codex_binary_path());

        // Set working directory if provided
        if let Some(cwd) = &config.cwd {
            cmd.cwd(cwd);
        }

        // Use mock-acp-agent model
        cmd.arg("--model");
        cmd.arg(&config.model);

        // Set approval policy if specified (also sets sandbox to allow test execution)
        if let Some(approval) = &config.approval_policy {
            cmd.arg("--ask-for-approval");
            cmd.arg(approval.as_str());
        }
        // Also set sandbox to workspace-write to allow file operations in tests
        if let Some(sandbox) = &config.sandbox {
            cmd.arg("--sandbox");
            cmd.arg(sandbox.as_str());
        }

        // Set TERM to enable terminal features
        cmd.env("TERM", "xterm-256color");

        // Set CODEX_HOME to temp directory if we have one, so logs and config
        // go to the temp directory instead of trying to write to ~/.codex
        if let Some(temp) = &temp_dir {
            cmd.env("CODEX_HOME", temp.path().to_str().unwrap());
        }

        // Pass through mock agent env vars
        for (key, value) in config.mock_agent_env {
            cmd.env(&key, &value);
        }

        // Disable color codes for easier parsing
        if config.no_color {
            cmd.env("NO_COLOR", "1");
        }

        let _child = pair.slave.spawn_command(cmd)?;

        // Set master PTY to non-blocking mode before cloning reader
        // This ensures the cloned reader FD inherits the non-blocking flag
        #[cfg(unix)]
        {
            if let Some(master_fd) = pair.master.as_raw_fd() {
                set_nonblocking(master_fd)?;
            }
        }

        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        Ok(Self {
            _master: pair.master,
            reader,
            writer,
            parser: Parser::new(rows, cols, 0),
            _temp_dir: temp_dir,
        })
    }

    /// Read any available output and update screen state
    ///
    /// This method attempts to read available data without blocking.
    /// It uses a simple approach of reading with a small buffer which works
    /// well for our polling-based test framework.
    pub fn poll(&mut self) -> Result<()> {
        // Create a small buffer for reading
        let mut buf = [0u8; 8192];

        if std::env::var("DEBUG_TUI_PTY").is_ok() {
            eprintln!("[DEBUG poll] About to call read()...");
        }
        let read_start = Instant::now();

        // The PTY reader is in non-blocking mode and will return immediately if no data is available
        // We rely on the polling loop in wait_for() to handle timing
        let read_result = self.reader.read(&mut buf);
        let read_duration = read_start.elapsed();

        if std::env::var("DEBUG_TUI_PTY").is_ok() {
            eprintln!("[DEBUG poll] read() returned after {:?}", read_duration);
        }

        match read_result {
            Ok(0) => {
                if std::env::var("DEBUG_TUI_PTY").is_ok() {
                    eprintln!("[DEBUG poll] read() returned Ok(0) - EOF/process exited");
                }
                Ok(())
            }
            Ok(n) => {
                if std::env::var("DEBUG_TUI_PTY").is_ok() {
                    eprintln!("[DEBUG poll] read() returned Ok({}) - {} bytes read", n, n);
                }
                // Intercept and respond to control sequences before parsing
                let processed = self.intercept_control_sequences(&buf[..n])?;
                self.parser.process(&processed);
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if std::env::var("DEBUG_TUI_PTY").is_ok() {
                    eprintln!("[DEBUG poll] read() returned WouldBlock - no data available");
                }
                Ok(())
            }
            Err(e) => {
                if std::env::var("DEBUG_TUI_PTY").is_ok() {
                    eprintln!("[DEBUG poll] read() returned Err: {}", e);
                }
                Err(e.into())
            }
        }
    }

    /// Intercept control sequences and inject responses
    ///
    /// Detects cursor position queries (ESC[6n) and writes responses back to the PTY
    /// Returns filtered data with control sequences removed
    fn intercept_control_sequences(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let mut result = Vec::with_capacity(data.len());
        let mut i = 0;

        while i < data.len() {
            // Detect cursor position query: ESC[6n
            if i + 3 < data.len()
                && data[i] == 0x1b      // ESC
                && data[i+1] == b'['
                && data[i+2] == b'6'
                && data[i+3] == b'n'
            {
                // Write response back to PTY: ESC[1;1R (cursor at row 1, col 1)
                self.writer.write_all(b"\x1b[1;1R")?;
                self.writer.flush()?;
                // Skip the control sequence - don't pass it to the parser
                i += 4;
            } else {
                result.push(data[i]);
                i += 1;
            }
        }
        Ok(result)
    }

    /// Wait for predicate with timeout
    pub fn wait_for<F>(&mut self, pred: F, timeout: Duration) -> Result<(), String>
    where
        F: Fn(&str) -> bool,
    {
        let debug = std::env::var("DEBUG_TUI_PTY").is_ok();
        if debug {
            eprintln!(
                "[DEBUG wait_for] Starting wait_for with timeout {:?}",
                timeout
            );
        }
        let start = Instant::now();
        let mut iteration = 0;

        loop {
            iteration += 1;
            let elapsed = start.elapsed();
            if debug {
                eprintln!(
                    "[DEBUG wait_for] Iteration {}, elapsed: {:?}",
                    iteration, elapsed
                );
                eprintln!("[DEBUG wait_for] Calling poll()...");
            }

            self.poll().map_err(|e| e.to_string())?;

            if debug {
                eprintln!("[DEBUG wait_for] poll() completed");
            }

            let contents = self.screen_contents();
            if debug {
                eprintln!(
                    "[DEBUG wait_for] Screen contents length: {} bytes",
                    contents.len()
                );
                eprintln!(
                    "[DEBUG wait_for] Screen contents preview: {:?}",
                    &contents.chars().take(100).collect::<String>()
                );
            }

            if pred(&contents) {
                if debug {
                    eprintln!(
                        "[DEBUG wait_for] Predicate matched! Success after {:?}",
                        elapsed
                    );
                }
                return Ok(());
            }

            if debug {
                eprintln!("[DEBUG wait_for] Predicate did not match");
            }

            if start.elapsed() > timeout {
                if debug {
                    eprintln!(
                        "[DEBUG wait_for] TIMEOUT REACHED after {:?}",
                        start.elapsed()
                    );
                }
                return Err(format!(
                    "Timeout waiting for condition.\nScreen contents:\n{}",
                    contents
                ));
            }

            if debug {
                eprintln!("[DEBUG wait_for] Sleeping 50ms before next iteration");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Wait for specific text to appear
    pub fn wait_for_text(&mut self, needle: &str, timeout: Duration) -> Result<(), String> {
        self.wait_for(|s| s.contains(needle), timeout)
    }

    /// Get current screen contents as string
    pub fn screen_contents(&self) -> String {
        self.parser.screen().contents()
    }

    /// Type a string
    pub fn send_str(&mut self, s: &str) -> std::io::Result<()> {
        self.writer.write_all(s.as_bytes())?;
        self.writer.flush()
    }

    /// Send a key event
    pub fn send_key(&mut self, key: Key) -> std::io::Result<()> {
        self.writer.write_all(&key.to_escape_sequence())?;
        self.writer.flush()
    }
}

/// Sandbox policy for codex session
#[derive(Debug, Clone, Copy)]
pub enum Sandbox {
    // [possible values: read-only, workspace-write, danger-full-access]
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
}

impl Sandbox {
    fn as_str(&self) -> &'static str {
        match self {
            Sandbox::ReadOnly => "read-only",
            Sandbox::WorkspaceWrite => "workspace-write",
            Sandbox::DangerFullAccess => "danger-full-access",
        }
    }
}

/// Approval policy for codex session
#[derive(Debug, Clone, Copy)]
pub enum ApprovalPolicy {
    /// Only run trusted commands without approval
    Untrusted,
    /// Run all commands, ask for approval on failure
    OnFailure,
    /// Model decides when to ask
    OnRequest,
    /// Never ask for approval
    Never,
}

impl ApprovalPolicy {
    fn as_str(&self) -> &'static str {
        match self {
            ApprovalPolicy::Untrusted => "untrusted",
            ApprovalPolicy::OnFailure => "on-failure",
            ApprovalPolicy::OnRequest => "on-request",
            ApprovalPolicy::Never => "never",
        }
    }
}

/// Configuration for spawning a test session
pub struct SessionConfig {
    pub model: String,
    pub mock_agent_env: HashMap<String, String>,
    pub no_color: bool,
    pub approval_policy: Option<ApprovalPolicy>,
    pub sandbox: Option<Sandbox>,
    pub cwd: Option<std::path::PathBuf>,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionConfig {
    pub fn new() -> Self {
        Self {
            model: "mock-acp".to_string(),
            mock_agent_env: HashMap::new(),
            no_color: true,
            approval_policy: Some(ApprovalPolicy::OnFailure),
            // [possible values: read-only, workspace-write, danger-full-access]
            sandbox: Some(Sandbox::WorkspaceWrite),
            cwd: None,
        }
    }

    pub fn with_mock_response(mut self, response: impl Into<String>) -> Self {
        self.mock_agent_env
            .insert("MOCK_AGENT_RESPONSE".to_string(), response.into());
        self
    }

    pub fn with_stream_until_cancel(mut self) -> Self {
        self.mock_agent_env.insert(
            "MOCK_AGENT_STREAM_UNTIL_CANCEL".to_string(),
            "1".to_string(),
        );
        self
    }

    pub fn with_agent_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.mock_agent_env.insert(key.into(), value.into());
        self
    }

    pub fn with_approval_policy(mut self, policy: ApprovalPolicy) -> Self {
        self.approval_policy = Some(policy);
        self
    }

    pub fn without_approval_policy(mut self) -> Self {
        self.approval_policy = None;
        self
    }

    pub fn with_sandbox(mut self, sandbox: Sandbox) -> Self {
        self.sandbox = Some(sandbox);
        self
    }

    pub fn without_sandbox(mut self) -> Self {
        self.sandbox = None;
        self
    }
}

/// Get path to codex binary
fn codex_binary_path() -> String {
    let test_exe = std::env::current_exe().expect("Failed to get current exe path");
    test_exe
        .parent() // deps
        .and_then(|p| p.parent()) // debug or release
        .expect("Failed to get target directory")
        .join("codex")
        .to_string_lossy()
        .into_owned()
}

pub const TIMEOUT: Duration = Duration::from_secs(5);

/// Normalize dynamic content in screen output for snapshot testing
pub fn normalize_for_snapshot(contents: String) -> String {
    let mut normalized = contents;

    // Replace /tmp/.tmpXXXXXX with placeholder
    if let Some(start) = normalized.find("/tmp/.tmp") {
        if let Some(end) = normalized[start..].find(char::is_whitespace) {
            normalized.replace_range(start..start + end, "[TMP_DIR]");
        }
    }

    // Replace dynamic prompt text on lines starting with ›
    let lines: Vec<String> = normalized
        .lines()
        .map(|line| {
            if line.trim_start().starts_with("› ")
                && (line.trim_start().starts_with("› Find and fix a bug")
                    || line.trim_start().starts_with("› Explain this codebase")
                    || line.trim_start().starts_with("› Write tests for")
                    || line.trim_start().starts_with("› Improve documentation")
                    || line.trim_start().starts_with("› Summarize recent commits")
                    || line.trim_start().starts_with("› Implement {feature}")
                    || line.contains("@filename"))
            {
                "› [DEFAULT_PROMPT]".to_string()
            } else {
                line.to_string()
            }
        })
        .collect();

    lines.join("\n")
}
