# Noridoc: execpolicy

Path: @/codex-rs/execpolicy

### Overview

The `codex-execpolicy` crate provides policy-based evaluation of shell commands to determine their safety before execution. It parses commands against a policy specification and classifies them as safe, unsafe, or requiring analysis. This is the first-generation policy engine.

### How it fits into the larger codebase

Execpolicy is used by core for command safety assessment:

- **Core** `command_safety/is_safe_command.rs` uses this for approval decisions
- **Sandbox assessment** checks commands before auto-approval
- **Complements** `execpolicy2` which is the newer policy engine

### Core Implementation

**Key Components:**

- `policy.rs`: Policy definition structures
- `policy_parser.rs`: Parses policy specifications
- `exec_call.rs`: Represents parsed command invocations
- `execv_checker.rs`: Main evaluation logic
- `valid_exec.rs`: Valid execution patterns

**Evaluation Flow:**

1. Parse command into structured representation
2. Match against policy rules
3. Resolve arguments against allowed patterns
4. Return safety classification

### Things to Know

**Policy Format:**

Policies define allowed command patterns with:
- Program name (literal or pattern)
- Argument types and constraints
- File path restrictions

**Argument Types:**

`arg_type.rs` and `arg_matcher.rs` handle:
- Literal values
- File paths with constraints
- Optional arguments
- Variadic arguments

**Special Commands:**

`sed_command.rs` provides special handling for sed commands due to their complex argument patterns.

**Build-time Policy:**

`build.rs` may embed default policies at compile time.

Created and maintained by Nori.
