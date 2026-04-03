# Noridoc: codex-execpolicy

Path: @/codex-rs/execpolicy

### Overview

The execpolicy crate provides parsing and evaluation of execution policies for command approval. Policies define which commands can be auto-approved based on patterns for executables, arguments, and their combinations.

### How it fits into the larger codebase

Used by `@/codex-rs/core/` (`command_safety/`) to determine whether shell commands require user approval or can be auto-executed.

### Core Implementation

**Policy Format** (`lib.rs`): Policies are defined as TOML:

```toml
[[rules]]
program = "git"
args = ["status", "log", "diff"]  # allowed subcommands

[[rules]]
program = "ls"
# no args restriction = all args allowed
```

**Evaluation** (`lib.rs`): The `ExecPolicy::evaluate()` method checks:
1. Program name matches a rule
2. Arguments match allowed patterns (if specified)
3. Returns `Allow` or `RequiresApproval`

**Pattern Matching**: Supports:
- Exact matches
- Glob patterns (via `wildmatch`)
- Argument prefixes

**Argument Types** (`arg_type.rs`, `arg_matcher.rs`):

- Literal values
- File paths with constraints
- Optional arguments
- Variadic arguments

**Special Commands:**

`sed_command.rs` provides special handling for sed commands due to their complex argument patterns.

### Things to Know

- Default policies are embedded for common safe commands (git status, ls, etc.)
- Custom policies can be specified in project configuration
- The policy is evaluated per-command, not per-session

Created and maintained by Nori.
