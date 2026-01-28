use anyhow::Result;
use owo_colors::OwoColorize;
use owo_colors::Style;
use portable_pty::CommandBuilder;
use portable_pty::PtySize;
use portable_pty::native_pty_system;
use std::collections::HashMap;
use std::io::IsTerminal;
use std::io::Read;
use std::io::Write;
use std::sync::LazyLock;
use std::time::Duration;
use std::time::Instant;
use vt100::Parser;

/// Debug styles for colored output. Uses owo-colors Style which respects
/// color settings - when colors are disabled, styles become no-ops.
struct DebugStyles {
    bold: Style,
    dim: Style,
    red: Style,
    green: Style,
    yellow: Style,
    blue: Style,
    magenta: Style,
    cyan: Style,
}

impl DebugStyles {
    fn new(with_color: bool) -> Self {
        if with_color {
            Self {
                bold: Style::new().bold(),
                dim: Style::new().dimmed(),
                red: Style::new().red(),
                green: Style::new().green(),
                yellow: Style::new().yellow(),
                blue: Style::new().blue(),
                magenta: Style::new().magenta(),
                cyan: Style::new().cyan(),
            }
        } else {
            Self {
                bold: Style::new(),
                dim: Style::new(),
                red: Style::new(),
                green: Style::new(),
                yellow: Style::new(),
                blue: Style::new(),
                magenta: Style::new(),
                cyan: Style::new(),
            }
        }
    }
}

static DEBUG_ENABLED: LazyLock<bool> = LazyLock::new(|| std::env::var("DEBUG_TUI_PTY").is_ok());

/// Color is enabled by default when stderr is a terminal, unless NO_COLOR is set
static DEBUG_STYLES: LazyLock<DebugStyles> = LazyLock::new(|| {
    let use_color = std::env::var("NO_COLOR").is_err() && std::io::stderr().is_terminal();
    DebugStyles::new(use_color)
});

fn debug_enabled() -> bool {
    *DEBUG_ENABLED
}

fn styles() -> &'static DebugStyles {
    &DEBUG_STYLES
}

fn indent_lines(text: &str, indent: &str) -> String {
    text.lines()
        .map(|line| format!("{indent}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

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
            let s = styles();
            let indent = "    ";

            // Header for screen state
            eprintln!(
                "\n{}",
                "=== TUI Screen State at Panic ==="
                    .style(s.bold)
                    .style(s.red)
            );

            // Screen contents with indentation
            let screen = self.screen_contents();
            eprintln!("{}", indent_lines(&screen, indent).style(s.cyan));

            if let Some(tmpdir) = &self._temp_dir {
                let log_tail = if let Some(log_path) = find_acp_log_file(tmpdir.path()) {
                    if let Ok(content) = std::fs::read_to_string(&log_path) {
                        let lines: Vec<&str> = content.lines().collect();
                        let start = lines.len().saturating_sub(150);
                        lines[start..].join("\n")
                    } else {
                        format!("<failed to read log file at {}>", log_path.display())
                    }
                } else {
                    "<no ACP log file found in NORI_HOME/log/>".to_string()
                };

                // Header for tracing
                eprintln!(
                    "\n{}",
                    "=== ACP Tracing Subscriber    ==="
                        .style(s.bold)
                        .style(s.yellow)
                );

                // Tracing content with indentation
                eprintln!("{}", indent_lines(&log_tail, indent).style(s.dim));
            }

            // Footer
            eprintln!(
                "{}",
                "================================="
                    .style(s.bold)
                    .style(s.red)
            );
            eprintln!();
        }
    }
}

impl TuiSession {
    /// Spawn codex using mock-acp-agent binary in a temporary directory
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

            // Initialize as git repo if requested (prevents "Snapshots disabled" race)
            // Use -b master to ensure consistent branch name regardless of system git config
            if config.git_init {
                std::process::Command::new("git")
                    .args(["init", "-b", "master"])
                    .current_dir(temp_dir.path())
                    .output()?;

                // Configure git user for the initial commit
                std::process::Command::new("git")
                    .args(["config", "user.email", "test@example.com"])
                    .current_dir(temp_dir.path())
                    .output()?;
                std::process::Command::new("git")
                    .args(["config", "user.name", "Test User"])
                    .current_dir(temp_dir.path())
                    .output()?;

                // Add and commit the hello.py file to create a branch
                std::process::Command::new("git")
                    .args(["add", "."])
                    .current_dir(temp_dir.path())
                    .output()?;
                std::process::Command::new("git")
                    .args(["commit", "-m", "Initial commit"])
                    .current_dir(temp_dir.path())
                    .output()?;
            }

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

        // Skip trust directory prompt for E2E tests (avoids interactive prompts)
        if config.skip_trust_directory {
            cmd.arg("--skip-trust-directory");
        }

        // Set TERM to enable terminal features
        cmd.env("TERM", "xterm-256color");

        // Set CODEX_HOME and NORI_HOME to temp directory if we have one, so logs
        // and config go to the temp directory instead of trying to write to
        // ~/.codex or ~/.nori/cli
        if let Some(temp) = &temp_dir {
            let codex_home = temp.path();
            cmd.env("CODEX_HOME", codex_home.to_str().unwrap());
            // Also set NORI_HOME for nori-config feature support
            cmd.env("NORI_HOME", codex_home.to_str().unwrap());

            // Write config.toml to CODEX_HOME (unless explicitly empty for first-launch testing)
            let config_path = codex_home.join("config.toml");
            let config_content = config.config_toml.clone().unwrap_or_else(|| {
                // Generate default config with model, trusted project path,
                // and mock_provider that doesn't require OpenAI auth.
                //
                // IMPORTANT: Canonicalize the cwd path for the projects section.
                // On macOS, /tmp is a symlink to /private/tmp, so paths like
                // /var/folders/... can become /private/var/folders/... after
                // canonicalization. The config loader canonicalizes CODEX_HOME
                // and resolved_cwd, so we must use the same canonicalized path
                // here to ensure the project trust level is properly matched.
                let cwd_path = config
                    .cwd
                    .as_ref()
                    .and_then(|p| std::fs::canonicalize(p).ok())
                    .map(|p| p.to_string_lossy().into_owned())
                    .or_else(|| {
                        config
                            .cwd
                            .as_ref()
                            .map(|p| p.to_string_lossy().into_owned())
                    })
                    .unwrap_or_else(|| codex_home.to_string_lossy().into_owned());
                let acp_section = if config.allow_http_fallback {
                    "\n[acp]\nallow_http_fallback = true\n"
                } else {
                    ""
                };
                format!(
                    r#"model = "{model}"
model_provider = "mock_provider"

[projects."{cwd}"]
trust_level = "trusted"

[model_providers.mock_provider]
name = "Mock ACP provider for tests"
{acp_section}"#,
                    model = config.model,
                    cwd = cwd_path,
                    acp_section = acp_section
                )
            });
            // Only write config file if content is non-empty. Empty string means
            // "no config file" which is needed to test the first-launch welcome screen.
            if !config_content.is_empty() {
                std::fs::write(&config_path, config_content)?;
            }
        }

        // Pass through mock agent env vars
        for (key, value) in config.mock_agent_env {
            cmd.env(&key, &value);
        }

        // Build PATH: filter excluded binaries, then prepend extra directories
        if !config.extra_path.is_empty() || !config.exclude_binaries.is_empty() {
            let current_path = std::env::var("PATH").unwrap_or_default();

            // Filter out directories containing excluded binaries
            let filtered_dirs: Vec<&str> = if config.exclude_binaries.is_empty() {
                current_path.split(':').collect()
            } else {
                current_path
                    .split(':')
                    .filter(|dir| {
                        !config
                            .exclude_binaries
                            .iter()
                            .any(|binary| std::path::Path::new(dir).join(binary).exists())
                    })
                    .collect()
            };

            // Prepend extra directories
            let extra_paths: Vec<String> = config
                .extra_path
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();

            let new_path = if extra_paths.is_empty() {
                filtered_dirs.join(":")
            } else {
                format!("{}:{}", extra_paths.join(":"), filtered_dirs.join(":"))
            };
            cmd.env("PATH", new_path);
        }

        // Disable color codes for easier parsing
        if config.no_color {
            cmd.env("NO_COLOR", "1");
        }

        // Force synchronous system info collection in E2E tests
        // This ensures footer displays git branch/nori version immediately
        cmd.env("NORI_SYNC_SYSTEM_INFO", "1");

        // Mock instruction files for consistent banner width across machines
        // This returns a constant list (~/.claude/CLAUDE.md) instead of discovering real files
        cmd.env("NORI_MOCK_INSTRUCTION_FILES", "1");

        // Enable debug logging for codex_acp to capture timing information
        cmd.env(
            "RUST_LOG",
            "codex_core=info,nori_tui=info,codex_rmcp_client=info,codex_acp=debug",
        );

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

        let debug = debug_enabled();
        let s = styles();

        if debug {
            eprintln!(
                "    {} About to call read()...",
                "[DEBUG poll]".style(s.blue)
            );
        }
        let read_start = Instant::now();

        // The PTY reader is in non-blocking mode and will return immediately if no data is available
        // We rely on the polling loop in wait_for() to handle timing
        let read_result = self.reader.read(&mut buf);
        let read_duration = read_start.elapsed();

        if debug {
            eprintln!(
                "    {} read() returned after {:?}",
                "[DEBUG poll]".style(s.blue),
                read_duration
            );
        }

        match read_result {
            Ok(0) => {
                if debug {
                    eprintln!(
                        "    {} read() returned {} - EOF/process exited",
                        "[DEBUG poll]".style(s.blue),
                        "Ok(0)".style(s.yellow)
                    );
                }
                Ok(())
            }
            Ok(n) => {
                if debug {
                    eprintln!(
                        "    {} read() returned {} - {} bytes read",
                        "[DEBUG poll]".style(s.blue),
                        format!("Ok({n})").style(s.green),
                        n
                    );
                }
                // Intercept and respond to control sequences before parsing
                let processed = self.intercept_control_sequences(&buf[..n])?;
                self.parser.process(&processed);
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if debug {
                    eprintln!(
                        "    {} read() returned {} - no data available",
                        "[DEBUG poll]".style(s.blue),
                        "WouldBlock".style(s.dim)
                    );
                }
                Ok(())
            }
            Err(e) => {
                if debug {
                    eprintln!(
                        "    {} read() returned {}",
                        "[DEBUG poll]".style(s.blue),
                        format!("Err: {e}").style(s.red)
                    );
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
        let debug = debug_enabled();
        let s = styles();

        if debug {
            eprintln!(
                "{} Starting wait_for with timeout {:?}",
                "[DEBUG wait_for]".style(s.magenta),
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
                    "{} Iteration {}, elapsed: {:?}",
                    "[DEBUG wait_for]".style(s.magenta),
                    iteration.style(s.cyan),
                    elapsed
                );
                eprintln!("{} Calling poll()...", "[DEBUG wait_for]".style(s.magenta));
            }

            self.poll().map_err(|e| e.to_string())?;

            if debug {
                eprintln!("{} poll() completed", "[DEBUG wait_for]".style(s.magenta));
            }

            let contents = self.screen_contents();
            if debug {
                eprintln!(
                    "{} Screen contents length: {} bytes",
                    "[DEBUG wait_for]".style(s.magenta),
                    contents.len()
                );
                eprintln!(
                    "{} Screen contents preview:",
                    "[DEBUG wait_for]".style(s.magenta)
                );
                let preview: String = contents.chars().take(100).collect();
                eprintln!("{}", indent_lines(&preview, "        ").style(s.dim));
            }

            if pred(&contents) {
                if debug {
                    eprintln!(
                        "{} {} Success after {:?}",
                        "[DEBUG wait_for]".style(s.magenta),
                        "Predicate matched!".style(s.green),
                        elapsed
                    );
                }
                return Ok(());
            }

            if debug {
                eprintln!(
                    "{} {}",
                    "[DEBUG wait_for]".style(s.magenta),
                    "Predicate did not match".style(s.yellow)
                );
            }

            if start.elapsed() > timeout {
                if debug {
                    eprintln!(
                        "{} {} after {:?}",
                        "[DEBUG wait_for]".style(s.magenta),
                        "TIMEOUT REACHED".style(s.red),
                        start.elapsed()
                    );
                }
                return Err("Timeout waiting for condition.".to_string());
            }

            if debug {
                eprintln!(
                    "{} Sleeping 50ms before next iteration",
                    "[DEBUG wait_for]".style(s.magenta)
                );
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

    /// Get the path to the ACP log file (if temp directory exists)
    ///
    /// This is useful for E2E tests that need to verify subprocess behavior
    /// by parsing the ACP tracing logs. Logs are stored in `$NORI_HOME/log/`
    /// with rolling daily naming: `nori-acp.YYYY-MM-DD`.
    pub fn acp_log_path(&self) -> Option<std::path::PathBuf> {
        self._temp_dir
            .as_ref()
            .and_then(|d| find_acp_log_file(d.path()))
    }

    /// Get the NORI_HOME path (temp directory used for config storage)
    ///
    /// This is useful for E2E tests that need to verify config.toml contents
    /// after user interactions (e.g., trust directory selection).
    pub fn nori_home_path(&self) -> Option<std::path::PathBuf> {
        self._temp_dir.as_ref().map(|d| d.path().to_path_buf())
    }
}

/// Find the ACP log file in the given NORI_HOME directory.
///
/// Searches for files matching `nori-acp.*` in the `log/` subdirectory,
/// returning the most recently modified one (handles rolling daily logs).
fn find_acp_log_file(nori_home: &std::path::Path) -> Option<std::path::PathBuf> {
    let log_dir = nori_home.join("log");
    if !log_dir.exists() {
        return None;
    }

    std::fs::read_dir(&log_dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("nori-acp."))
        })
        .max_by_key(|entry| entry.metadata().ok().and_then(|m| m.modified().ok()))
        .map(|entry| entry.path())
}

/// Configuration for spawning a test session
pub struct SessionConfig {
    pub model: String,
    pub mock_agent_env: HashMap<String, String>,
    pub no_color: bool,
    /// Skip the trust directory prompt (passes --skip-trust-directory flag).
    /// Enabled by default for E2E tests to avoid interactive prompts.
    pub skip_trust_directory: bool,
    pub cwd: Option<std::path::PathBuf>,
    /// Custom config.toml content. If None, a default config will be generated.
    /// Set to Some("") to write an empty config file.
    pub config_toml: Option<String>,
    /// Initialize the temp directory as a git repository.
    /// This prevents the "Snapshots disabled" BackgroundEvent from overwriting
    /// the "Working" status indicator during streaming tests.
    pub git_init: bool,
    /// When true, allows falling back to HTTP providers if model is not in ACP registry.
    /// When false (default), ACP-only mode: unregistered models produce an error.
    pub allow_http_fallback: bool,
    /// Extra directories to prepend to PATH when spawning the process.
    pub extra_path: Vec<std::path::PathBuf>,
    /// Binary names to exclude from PATH (filters out directories containing these binaries).
    /// Useful for testing behavior when certain commands are "not installed".
    pub exclude_binaries: Vec<String>,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionConfig {
    pub fn new() -> Self {
        Self {
            model: "mock-model".to_string(),
            mock_agent_env: HashMap::new(),
            no_color: true,
            skip_trust_directory: true, // Skip trust prompt by default for E2E tests
            cwd: None,
            config_toml: None,
            git_init: true,
            allow_http_fallback: false, // Default to ACP-only mode for tests
            extra_path: Vec::new(),
            // Exclude installers by default since it won't be in PATH on CI runners.
            // Tests that need installers should explicitly add it via with_extra_path().
            exclude_binaries: vec!["nori-ai".to_string(), "nori-skillsets".to_string()],
        }
    }

    pub fn with_allow_http_fallback(mut self, allow_http_fallback: bool) -> Self {
        self.allow_http_fallback = allow_http_fallback;
        self
    }

    pub fn with_model(mut self, model: String) -> Self {
        self.model = model;
        self
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

    /// Configure the mock agent to send a tool call sequence during the prompt
    pub fn with_tool_call(mut self) -> Self {
        self.mock_agent_env
            .insert("MOCK_AGENT_SEND_TOOL_CALL".to_string(), "1".to_string());
        self
    }

    pub fn with_agent_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.mock_agent_env.insert(key.into(), value.into());
        self
    }

    /// Enable or disable the --skip-trust-directory flag.
    /// Enabled by default; use `with_skip_trust_directory(false)` to test trust prompts.
    pub fn with_skip_trust_directory(mut self, skip: bool) -> Self {
        self.skip_trust_directory = skip;
        self
    }

    pub fn with_config_toml(mut self, content: impl Into<String>) -> Self {
        self.config_toml = Some(content.into());
        self
    }

    /// Initialize the temp directory as a git repository.
    pub fn without_git_init(mut self) -> Self {
        // This prevents the "Snapshots disabled" BackgroundEvent from racing
        // with the "Working" status indicator during streaming tests.
        self.git_init = false;
        self
    }

    /// Add an extra directory to prepend to PATH when spawning the process.
    pub fn with_extra_path(mut self, path: std::path::PathBuf) -> Self {
        self.extra_path.push(path);
        self
    }

    /// Exclude directories containing a specific binary from PATH.
    /// Useful for testing behavior when certain commands are "not installed".
    pub fn with_excluded_binary(mut self, binary_name: impl Into<String>) -> Self {
        self.exclude_binaries.push(binary_name.into());
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
        .join("nori")
        .to_string_lossy()
        .into_owned()
}

pub const TIMEOUT: Duration = Duration::from_secs(10);
pub const TIMEOUT_INPUT: Duration = Duration::from_millis(100);
pub const TIMEOUT_PRESNAPSHOT: Duration = Duration::from_millis(1000);

/// Replace a dynamic value after a marker, preserving box alignment
fn replace_after_marker(line: &str, marker: &str, replacement: &str) -> Option<String> {
    let start = line.find(marker)?;
    let val_start = start + marker.len();
    let rest = &line[val_start..];

    // Skip leading whitespace to find where value begins
    let val_offset = rest.find(|c: char| !c.is_whitespace()).unwrap_or(0);
    let val_start = val_start + val_offset;

    // Find value end (next whitespace) and region end (│ border or EOL)
    let rest = &line[val_start..];
    let val_end = rest
        .find(char::is_whitespace)
        .map_or(line.len(), |pos| val_start + pos);
    let region_end = rest.find('│').map_or(line.len(), |pos| val_start + pos);

    // Check that we have a non-empty value
    if val_start >= val_end {
        return None;
    }

    // Replace value with placeholder, padding to maintain width
    let mut result = line.to_string();
    let region_width = region_end - val_start;
    if region_width >= replacement.len() {
        let padded = format!(
            "{}{}",
            replacement,
            " ".repeat(region_width - replacement.len())
        );
        result.replace_range(val_start..region_end, &padded);
    } else {
        result.replace_range(val_start..val_end, replacement);
    }
    Some(result)
}

/// Normalize dynamic content in screen output for snapshot testing
pub fn normalize_for_snapshot(contents: String) -> String {
    let mut normalized = contents;

    // Replace temp directories: /tmp/claude/.tmpXXXXXX or /tmp/.tmpXXXXXX -> [TMP_DIR]
    for pattern in &["/tmp/claude/.tmp", "/tmp/.tmp"] {
        while let Some(start) = normalized.find(pattern) {
            let end = normalized[start..]
                .find(|c: char| c.is_whitespace() || c == '│')
                .map_or(normalized.len(), |pos| start + pos);
            normalized.replace_range(start..end, "[TMP_DIR]");
        }
    }

    // Replace nix-shell temp directories: /tmp/nix-shell.XXXX/.tmpXXXXXX -> [TMP_DIR]
    // This handles the case where nix creates nested tmp directories
    while let Some(start) = normalized.find("/tmp/nix-shell.") {
        let end = normalized[start..]
            .find(|c: char| c.is_whitespace() || c == '│')
            .map_or(normalized.len(), |pos| start + pos);
        normalized.replace_range(start..end, "[TMP_DIR]");
    }

    // Per-line replacements
    let lines: Vec<String> = normalized
        .lines()
        .map(|line| {
            let mut line = line.to_string();

            // Normalize "─ Worked for Xs ───..." timing lines to solid bars
            // This prevents flaky tests due to variable timing
            if line.starts_with("─ Worked") && line.ends_with('─') {
                return "─".repeat(line.chars().count());
            }

            // Version: "Nori CLI vX.Y.Z-prerelease" -> "Nori CLI v0.0.0"
            // Only replace if it looks like a version (digit after "Nori CLI v")
            let is_version = line
                .find("Nori CLI v")
                .and_then(|pos| line.chars().nth(pos + 10))
                .is_some_and(|c| c.is_ascii_digit());
            if is_version && let Some(result) = replace_after_marker(&line, "Nori CLI ", "v0.0.0") {
                line = result;
            }

            // Profile: "profile:   value" -> "profile:   [PROF]"
            if let Some(result) = replace_after_marker(&line, "profile:", "[PROF]") {
                line = result;
            }

            // Session ID: "Session: 019bb411-..." -> "Session: [SESSION_ID]"
            if let Some(result) = replace_after_marker(&line, "Session:", "[SESSION_ID]") {
                line = result;
            }

            line
        })
        .collect();
    normalized = lines.join("\n");

    // Normalize box line widths to a fixed width (prevents flaky snapshots from varying content)
    // This handles lines like "│ content │" and "╰───────╯" that vary based on directory path length
    const FIXED_BOX_WIDTH: usize = 37; // Fixed inner width for consistency
    let lines: Vec<String> = normalized
        .lines()
        .map(|line| {
            // Normalize box content lines: "│ content    │" -> fixed width
            if line.starts_with('│') && line.ends_with('│') {
                let inner = &line[3..line.len() - 3]; // Strip "│ " and " │" (3 bytes each for UTF-8)
                let trimmed = inner.trim_end();
                if trimmed.len() < FIXED_BOX_WIDTH {
                    return format!("│ {:<width$} │", trimmed, width = FIXED_BOX_WIDTH);
                }
            }
            // Normalize bottom border: "╰───────╯" -> fixed width
            if line.starts_with('╰')
                && line.ends_with('╯')
                && line.chars().all(|c| c == '╰' || c == '─' || c == '╯')
            {
                return format!("╰{}╯", "─".repeat(FIXED_BOX_WIDTH + 2));
            }
            line.to_string()
        })
        .collect();
    normalized = lines.join("\n");

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

/// Normalize for input tests - strips header for consistent snapshot regardless of scroll state
pub fn normalize_for_input_snapshot(contents: String) -> String {
    // Capture if original input has trailing newline before normalize_for_snapshot strips it
    let has_trailing_newline = contents.ends_with('\n');
    let normalized = normalize_for_snapshot(contents);

    // Strip ACP error messages (prevents flaky snapshots due to timing of debug-mode errors)
    // Pattern: "■ Operation 'X' is not supported in ACP mode" followed by optional empty line
    let lines: Vec<&str> = normalized.lines().collect();
    let mut filtered_lines = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        if line.contains("■ Operation '") && line.contains("' is not supported in ACP mode") {
            // Skip the error line
            i += 1;
            // If next line is empty, skip it too
            if i < lines.len() && lines[i].trim().is_empty() {
                i += 1;
            }
        } else {
            filtered_lines.push(line);
            i += 1;
        }
    }
    let normalized = filtered_lines.join("\n");

    // Strip startup header block if present (prevents flaky snapshots due to scroll timing)
    // The header can appear in two forms:
    // 1. Boxed header with "╭──" border
    // 2. Plain text "Nori CLI"
    // The header ends with either:
    // - nori-skillsets init command (when nori-skillsets is not installed)
    // - bottom border "╰──" (when nori-skillsets is already installed)
    let lines: Vec<&str> = normalized.lines().collect();

    // Detect if header is present (either boxed or plain text form)
    let has_header = lines.iter().any(|l| {
        l.contains("╭──") || l.contains("Nori CLI") || l.contains("'npx nori-skillsets init'")
    });

    let mut result = if has_header {
        // Find where the header ends
        let mut skip_until = 0;
        for (i, line) in lines.iter().enumerate() {
            // The nori-skillsets init line marks the end of the command list (if present)
            if line.contains("'npx nori-skillsets init'") {
                skip_until = i + 1;
                break;
            }
            // If no install line, use the bottom border as the end marker
            if line.contains("╰──") {
                skip_until = i + 1;
                // Don't break yet - install line may follow
            }
        }
        // Skip empty lines after the header block
        while skip_until < lines.len() && lines[skip_until].trim().is_empty() {
            skip_until += 1;
        }
        if skip_until > 0 {
            lines
                .into_iter()
                .skip(skip_until)
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            normalized
        }
    } else {
        normalized
    };

    // Restore trailing newline if original input had one
    if has_trailing_newline && !result.is_empty() {
        result.push('\n');
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_worked_for_line() {
        // Test that "─ Worked for Xs ───..." lines are normalized to solid bars
        let input =
            "─ Worked for 0s ────────────────────────────────────────────────────────────────";
        let expected =
            "────────────────────────────────────────────────────────────────────────────────";
        assert_eq!(normalize_for_snapshot(input.to_string()), expected);

        // Test with different timing values
        let input_1s =
            "─ Worked for 1s ────────────────────────────────────────────────────────────────";
        assert_eq!(normalize_for_snapshot(input_1s.to_string()), expected);

        let input_10s =
            "─ Worked for 10s ───────────────────────────────────────────────────────────────";
        assert_eq!(normalize_for_snapshot(input_10s.to_string()), expected);

        // Test with minute format (e.g., "1m 30s")
        let input_1m30s =
            "─ Worked for 1m 30s ────────────────────────────────────────────────────────────";
        assert_eq!(normalize_for_snapshot(input_1m30s.to_string()), expected);

        // Test that non-matching lines are unchanged
        let regular_line = "This is a regular line of text";
        assert_eq!(
            normalize_for_snapshot(regular_line.to_string()),
            regular_line
        );

        // Test partial match - doesn't start with ─
        let partial =
            "Worked for 0s ────────────────────────────────────────────────────────────────";
        assert_eq!(normalize_for_snapshot(partial.to_string()), partial);

        // Test within a multi-line content
        let multi_line = "Some content\n─ Worked for 5s ────────────────────────────────────────────────────────────────\nMore content";
        let expected_multi = "Some content\n────────────────────────────────────────────────────────────────────────────────\nMore content";
        assert_eq!(
            normalize_for_snapshot(multi_line.to_string()),
            expected_multi
        );
    }

    #[test]
    fn test_normalize_acp_error_messages() {
        // Test ACP error at start with empty line after
        let input =
            "■ Operation 'ListCustomPrompts' is not supported in ACP mode\n\n› [DEFAULT_PROMPT]\n";
        let expected = "› [DEFAULT_PROMPT]\n";
        assert_eq!(normalize_for_input_snapshot(input.to_string()), expected);

        // Test ACP error at start without empty line after
        let input_no_empty =
            "■ Operation 'ListCustomPrompts' is not supported in ACP mode\n› [DEFAULT_PROMPT]\n";
        let expected_no_empty = "› [DEFAULT_PROMPT]\n";
        assert_eq!(
            normalize_for_input_snapshot(input_no_empty.to_string()),
            expected_no_empty
        );

        // Test ACP error in middle of content
        let input_middle = "Some content\n■ Operation 'AddToHistory' is not supported in ACP mode\n\nMore content\n";
        let expected_middle = "Some content\nMore content\n";
        assert_eq!(
            normalize_for_input_snapshot(input_middle.to_string()),
            expected_middle
        );

        // Test multiple ACP errors
        let input_multiple = "■ Operation 'ListCustomPrompts' is not supported in ACP mode\n\n■ Operation 'AddToHistory' is not supported in ACP mode\n\nContent\n";
        let expected_multiple = "Content\n";
        assert_eq!(
            normalize_for_input_snapshot(input_multiple.to_string()),
            expected_multiple
        );

        // Test content without ACP errors (no-op)
        let input_no_errors = "› [DEFAULT_PROMPT]\n\n  ⎇ master · Nori v0.0.0\n";
        assert_eq!(
            normalize_for_input_snapshot(input_no_errors.to_string()),
            input_no_errors
        );

        // Test ACP error at end with no trailing newline
        let input_at_end = "Content\n■ Operation 'Foo' is not supported in ACP mode";
        let expected_at_end = "Content";
        assert_eq!(
            normalize_for_input_snapshot(input_at_end.to_string()),
            expected_at_end
        );

        // Test multiple consecutive empty lines after error (only strip one)
        let input_multiple_empty = "■ Operation 'Bar' is not supported in ACP mode\n\n\nContent\n";
        let expected_multiple_empty = "\nContent\n";
        assert_eq!(
            normalize_for_input_snapshot(input_multiple_empty.to_string()),
            expected_multiple_empty
        );

        // Test different operation names
        let input_diff_op =
            "■ Operation 'SomeOtherOperation' is not supported in ACP mode\n\nContent\n";
        let expected_diff_op = "Content\n";
        assert_eq!(
            normalize_for_input_snapshot(input_diff_op.to_string()),
            expected_diff_op
        );

        // Test that similar but non-matching text is preserved
        let input_similar = "■ This is some other message\n\nContent\n";
        assert_eq!(
            normalize_for_input_snapshot(input_similar.to_string()),
            input_similar
        );
    }

    #[test]
    fn test_normalize_version_and_profile() {
        // Test that version and profile are normalized correctly
        let input = r#"╭──────────────────────────────────────────────────────────────╮
│ Nori CLI v0.1.2                                              │
│ profile:   testuser                                          │
╰──────────────────────────────────────────────────────────────╯"#;

        let normalized = normalize_for_snapshot(input.to_string());

        // Profile should be normalized to [PROF]
        assert!(
            normalized.contains("[PROF]"),
            "Profile should be normalized, got:\n{}",
            normalized
        );

        // Version should be normalized to v0.0.0 (Nori CLI vX.Y.Z format)
        assert!(
            normalized.contains("Nori CLI v0.0.0"),
            "Version should be normalized to 'Nori CLI v0.0.0', got:\n{}",
            normalized
        );

        // Box structure preserved
        assert!(
            normalized.contains("╭──"),
            "Should preserve top border, got:\n{}",
            normalized
        );
        assert!(
            normalized.contains("╰──"),
            "Should preserve bottom border, got:\n{}",
            normalized
        );
    }
}
