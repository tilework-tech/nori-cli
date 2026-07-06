# Noridoc: codex-execpolicy

Path: @/nori-rs/execpolicy

### Overview

The execpolicy crate provides parsing and evaluation of execution policies for command approval. Policies define which commands can be auto-approved based on patterns for executables, arguments, and their combinations.

### How it fits into the larger codebase

Its only remaining consumer is the `nori execpolicycheck` debug subcommand in `@/nori-rs/cli/src/main.rs`, which checks policy files against a command. The former runtime consumer -- `codex-core`'s `command_safety/` auto-approval module -- was deleted in the crate-layering cleanup (`@/docs/specs/crate-layering.md`), so this crate is no longer on the live approval path.

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
