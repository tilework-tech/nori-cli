use std::io::Stdout;
use std::time::Duration;

use anyhow::Result;
use crossterm::event;
use crossterm::execute;
use crossterm::terminal::EnterAlternateScreen;
use crossterm::terminal::LeaveAlternateScreen;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;
use nori_tui_components::Theme;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

pub struct StorybookTerminal {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
    pub theme: Theme,
}

impl StorybookTerminal {
    pub fn enter() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        let theme = Theme::for_terminal_background(relative_terminal_background());
        Ok(Self { terminal, theme })
    }

    pub fn next_event(&mut self) -> Result<Option<event::Event>> {
        if event::poll(Duration::from_millis(100))? {
            Ok(Some(event::read()?))
        } else {
            Ok(None)
        }
    }
}

#[cfg(unix)]
fn relative_terminal_background() -> Option<(u8, u8, u8)> {
    use crossterm::style::Color;
    use crossterm::style::query_background_color;

    supports_color::on_cached(supports_color::Stream::Stdout).filter(|level| level.has_16m)?;
    match query_background_color().ok().flatten()? {
        Color::Rgb { r, g, b } => Some((r, g, b)),
        _ => None,
    }
}

#[cfg(not(unix))]
fn relative_terminal_background() -> Option<(u8, u8, u8)> {
    None
}

impl Drop for StorybookTerminal {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}
