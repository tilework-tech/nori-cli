# Noridoc: rust-hello-world

Path: @/.worktrees/rust-hello-world

### Overview

- Binary Rust package serving as a minimal "Hello World" example
- Verifies Rust toolchain installation and basic project setup (rustc 1.91.0, cargo 1.91.0)
- Uses standard Cargo project structure with src/main.rs entry point
- Edition 2024 - latest Rust edition available

### How it fits into the larger codebase

- Located in a git worktree (separate working directory) to maintain isolation from the main monorepo
- Does not affect main repository commits since .worktrees/ is gitignored at @/.gitignore
- Demonstrates the first Rust infrastructure setup for the Nori system
- Serves as a template for future Rust packages or can be replaced with actual project code
- No dependencies on other parts of the monorepo; entirely self-contained

### Core Implementation

- Entry point: src/main.rs contains a simple println!("Hello, world!") implementation
- Build configuration: Cargo.toml defines package metadata (name: rust-hello-world, version: 0.1.0, edition: 2024)
- Build output directory: target/ (gitignored via local .gitignore)
- Cargo.lock tracks dependency versions (currently no external dependencies)
- No dependencies defined, pure standard library usage

### Things to Know

- This is a binary crate (executable), not a library crate
- The gitignored .worktrees/ directory means this package does not integrate with the main repository version control
- Rust 1.91.0 is the minimum required toolchain - defined by rust-toolchain.toml if present or system default
- target/ build artifacts are local to the worktree and gitignored
- The package can be compiled with `cargo build` or run directly with `cargo run`

Created and maintained by Nori.
