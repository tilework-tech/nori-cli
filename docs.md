# Noridoc: nori-cli

Path: @/.worktrees/ratatui-hello-world

### Overview

- Binary Rust package implementing a terminal user interface (TUI) using the ratatui framework
- Foundation for building interactive terminal applications within the Nori system
- Uses standard Cargo project structure with src/main.rs entry point
- Edition 2024 - latest Rust edition available

### How it fits into the larger codebase

- Located in a git worktree (separate working directory) to maintain isolation from the main monorepo
- Does not affect main repository commits since .worktrees/ is gitignored at @/.gitignore
- Establishes TUI infrastructure that future Nori CLI features will build upon
- Self-contained application with no dependencies on other parts of the monorepo
- Demonstrates integration of third-party TUI libraries into the Nori ecosystem

### Core Implementation

- Entry point: src/main.rs implements ratatui's three-phase application pattern:
  1. Initialization: `ratatui::init()` configures terminal for TUI mode
  2. Event loop: `run()` function handles user input and rendering cycles
  3. Cleanup: `ratatui::restore()` returns terminal to normal mode
- Build configuration: Cargo.toml defines package metadata (name: nori-cli, version: 0.1.0, edition: 2024)
- Dependencies:
  - ratatui 0.29.0 - TUI framework providing widgets, layout, and rendering
  - crossterm 0.28.1 - Cross-platform terminal manipulation (used by ratatui)
  - color-eyre 0.6.3 - Enhanced error reporting with backtraces
- Rendering architecture: `render(frame: &mut Frame)` function separated from event loop for clean separation of concerns
- Text rendering uses ratatui's hierarchy: Span (styled text fragments) → Line (collection of spans) → Paragraph (widget)
- Event handling: blocking `crossterm::event::read()` waits for user input, exits on any key press
- Build output directory: target/ (gitignored via local .gitignore)
- Cargo.lock tracks dependency versions for reproducible builds

### Things to Know

- This is a binary crate (executable), not a library crate
- Terminal lifecycle management is critical: `ratatui::init()` must be paired with `ratatui::restore()` to avoid leaving terminal in broken state
- Error handling uses color-eyre's Result type, which must be installed in main via `color_eyre::install()?`
- The application runs in raw mode: terminal input is not line-buffered, all key events are captured immediately
- Current implementation exits on ANY key press - future versions will need proper event dispatch for different keys
- Rendering is blocking: `terminal.draw()` only returns after the render function completes
- Event reading is blocking: `event::read()?` halts execution until an event occurs
- The gitignored .worktrees/ directory means this package does not integrate with the main repository version control
- Rust 1.91.0 is the minimum required toolchain - defined by rust-toolchain.toml if present or system default
- target/ build artifacts are local to the worktree and gitignored
- The package can be compiled with `cargo build` or run directly with `cargo run`
- CI/CD: Separate GitHub Actions workflows for different Git events:
  - @/.github/workflows/pr-ci.yml runs on pull requests
  - @/.github/workflows/main-ci.yml runs on pushes to main
  - Both run: cargo fmt --check (formatting), cargo clippy -- -D warnings (linting), cargo test --verbose (tests)

Created and maintained by Nori.
