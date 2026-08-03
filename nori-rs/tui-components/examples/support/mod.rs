use std::io::Stdout;
use std::time::Duration;

use anyhow::Result;
use crossterm::event;
use crossterm::execute;
use crossterm::terminal::EnterAlternateScreen;
use crossterm::terminal::LeaveAlternateScreen;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

pub struct StorybookTerminal {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl StorybookTerminal {
    pub fn enter() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        Ok(Self { terminal })
    }

    pub fn next_event(&mut self) -> Result<Option<event::Event>> {
        if event::poll(Duration::from_millis(100))? {
            Ok(Some(event::read()?))
        } else {
            Ok(None)
        }
    }
}

impl Drop for StorybookTerminal {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}
