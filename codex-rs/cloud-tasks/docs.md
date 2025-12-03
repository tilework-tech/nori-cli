# Noridoc: cloud-tasks

Path: @/codex-rs/cloud-tasks

### Overview

The `codex-cloud-tasks` crate provides a TUI for browsing and managing Codex Cloud tasks. It allows users to view tasks from the web-based Codex interface and apply their changes locally.

### How it fits into the larger codebase

Cloud tasks is invoked via `codex cloud`:

- **Uses** `cloud-tasks-client` for API communication
- **Uses** Ratatui for TUI display
- **Integrates** with local git for applying diffs

### Core Implementation

**Key Files:**

- `app.rs`: Main application state and event loop
- `ui.rs`: TUI rendering
- `cli.rs`: Command-line argument parsing
- `scrollable_diff.rs`: Scrollable diff viewer widget
- `new_task.rs`: Task creation flow
- `env_detect.rs`: Environment detection for task context

### Things to Know

**Functionality:**

- List cloud tasks with filtering
- View task details and diffs
- Apply task changes to local repository
- Create new tasks from CLI

**Environment Detection:**

`env_detect.rs` detects repository context for task creation:
- Git remote URLs
- Branch information
- Working directory

Created and maintained by Nori.
