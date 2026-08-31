//! Test-only adapter for CSRessel's isolated tmux scripts. No browser or event
//! loop belongs here: run a real example, capture its display rows, then compare.
#![allow(dead_code, unused_macros, unused_imports)]

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;

use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;

pub struct Screen {
    pub ansi: String,
    pub text: String,
}

impl Screen {
    pub fn from_ansi(ansi: String) -> Result<Self> {
        // capture-pane -e emits display rows with SGR, not a raw PTY stream.
        // Reject other controls rather than silently interpreting them wrongly.
        let mut text = String::new();
        let mut chars = ansi.chars();
        while let Some(character) = chars.next() {
            if character != '\x1b' {
                ensure!(
                    !character.is_control() || character == '\n',
                    "unexpected control in tmux display rows"
                );
                text.push(character);
                continue;
            }
            ensure!(
                chars.next() == Some('['),
                "expected SGR in tmux display rows"
            );
            loop {
                match chars.next() {
                    Some('m') => break,
                    Some('0'..='9' | ';' | ':') => {}
                    _ => anyhow::bail!("unsupported or incomplete SGR in tmux display rows"),
                }
            }
        }
        Ok(Self { text, ansi })
    }

    pub fn snapshot_ansi(&self) -> String {
        self.ansi.replace('\\', "\\\\").replace('\x1b', "\\x1b")
    }

    pub fn replay_ansi(&self) -> String {
        // The last LF terminates a captured display row; replaying it at the
        // bottom of the grid would scroll away the first row. Keep blank rows.
        let rows = self.ansi.strip_suffix('\n').unwrap_or(&self.ansi);
        format!("\x1b[?25l{rows}")
    }
}

pub struct TuiSession {
    scripts: PathBuf,
    name: String,
    directory: PathBuf,
    example: String,
    cols: i32,
    rows: i32,
}

impl TuiSession {
    pub fn start(example: &str, cols: i32, rows: i32) -> Result<Self> {
        ensure!(cols > 0 && rows > 0, "terminal dimensions must be positive");
        let scripts = PathBuf::from(
            std::env::var_os("TUI_PUPPETEERING_DIR")
                .context("set TUI_PUPPETEERING_DIR to the installed tmux skill")?,
        )
        .canonicalize()?;
        for script in [
            "tui-start",
            "tui-send",
            "tui-assert",
            "tui-capture",
            "tui-stop",
            "tmux-isolated",
        ] {
            ensure!(
                scripts.join(script).is_file(),
                "missing TUI script: {script}"
            );
        }
        let binary = PathBuf::from(
            std::env::var_os("NORI_STORYBOOK_BIN_DIR")
                .context("run scripts/storybook-e2e.sh to build and locate the examples")?,
        )
        .join(example);
        ensure!(
            binary.is_file(),
            "storybook binary missing: {}",
            binary.display()
        );
        static NEXT_SESSION: AtomicI32 = AtomicI32::new(0);
        let time = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_nanos();
        let sequence = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
        let name = format!("nori-storybook-{}-{time}-{sequence}", std::process::id());
        let directory = std::env::temp_dir().join(&name);
        std::fs::create_dir(&directory)?;
        // Construct the guard before starting: setup failures also stop this
        // exact session. Never stop another test's session or the tmux server.
        let session = Self {
            scripts,
            name,
            directory,
            example: example.to_owned(),
            cols,
            rows,
        };
        let quoted_binary = binary.to_string_lossy().replace('\'', "'\\''");
        let command = format!(
            "env -u NO_COLOR TERM=xterm-256color COLORTERM=truecolor LC_ALL=en_US.UTF-8 '{quoted_binary}'"
        );
        session.run("tui-start", &[&session.name, "/bin/sh"])?;
        session.run(
            "tmux-isolated",
            &[
                "resize-window",
                "-t",
                &session.name,
                "-x",
                &cols.to_string(),
                "-y",
                &rows.to_string(),
            ],
        )?;
        session.run(
            "tmux-isolated",
            &[
                "set-option",
                "-p",
                "-t",
                &session.name,
                "window-style",
                "fg=#dde1e6,bg=#161616",
            ],
        )?;
        session.send(&format!("exec {command}"))?;
        session.key("Enter")?;
        Ok(session)
    }

    pub fn session_name(&self) -> &str {
        &self.name
    }

    pub fn expect(&self, text: &str) -> Result<()> {
        self.run("tui-assert", &[&self.name, text, "10"])?;
        Ok(())
    }

    pub fn send(&self, text: &str) -> Result<()> {
        self.run("tui-send", &[&self.name, text])?;
        Ok(())
    }

    pub fn key(&self, key: &str) -> Result<()> {
        self.run("tui-send", &[&self.name, "--keys", key])?;
        Ok(())
    }

    pub fn capture(&self, name: &str) -> Result<Screen> {
        // Wait for unchanged display rows, not a fixed delay after input.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut previous = self.run("tui-capture", &[&self.name, "-e"])?;
        let ansi = loop {
            std::thread::sleep(Duration::from_millis(100));
            let next = self.run("tui-capture", &[&self.name, "-e"])?;
            if next == previous {
                break next;
            }
            ensure!(Instant::now() < deadline, "storybook screen did not settle");
            previous = next;
        };
        let screen = Screen::from_ansi(ansi)?;
        ensure!(
            screen.ansi.contains("\x1b["),
            "expected a styled storybook capture"
        );
        let root = PathBuf::from(
            std::env::var_os("NORI_STORYBOOK_ARTIFACT_DIR")
                .context("run scripts/storybook-e2e.sh to set the capture directory")?,
        );
        let artifact = root.join(&self.example).join(name);
        std::fs::create_dir_all(&artifact)?;
        std::fs::write(artifact.join("screen.ansi"), &screen.ansi)?;
        std::fs::write(artifact.join("replay.ansi"), screen.replay_ansi())?;
        std::fs::write(artifact.join("screen.txt"), &screen.text)?;
        std::fs::write(
            artifact.join("geometry.txt"),
            format!("{} {}\n", self.cols, self.rows),
        )?;
        Ok(screen)
    }

    fn run(&self, script: &str, args: &[&str]) -> Result<String> {
        let output = Command::new(self.scripts.join(script))
            .args(args)
            .current_dir(&self.directory)
            .output()
            .with_context(|| format!("run {script}"))?;
        ensure!(
            output.status.success(),
            "{script} failed: {}\n{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr),
            std::fs::read_to_string(self.directory.join(format!("{}_failure.log", self.name)))
                .unwrap_or_default()
        );
        String::from_utf8(output.stdout).context("TUI script returned non-UTF-8 output")
    }
}

impl Drop for TuiSession {
    fn drop(&mut self) {
        if let Err(error) = self.run("tui-stop", &[&self.name]) {
            eprintln!("storybook cleanup: {error:#}");
        }
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

macro_rules! assert_screen {
    ($session:expr, $name:literal) => {{
        let screen = $session.capture($name)?;
        // Preserve blank rows and trailing cells without trailing whitespace in
        // the snapshot file itself. The bars are not part of the captured UI.
        let frame = |text: &str| text.lines().map(|line| format!("│{line}│")).collect::<Vec<_>>().join("\n");
        insta::with_settings!({ omit_expression => true }, {
            insta::assert_snapshot!(concat!($name, "_text"), frame(&screen.text));
            insta::assert_snapshot!(concat!($name, "_ansi"), frame(&screen.snapshot_ansi()));
        });
    }};
}
pub(crate) use assert_screen;
