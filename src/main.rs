mod app;
mod ui;
mod backends;

use color_eyre::Result;

fn main() -> Result<()> {
    color_eyre::install()?;
    println!("Agent Router TUI - Coming soon!");
    Ok(())
}
