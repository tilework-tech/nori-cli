# Noridoc: execpolicy2

Path: @/codex-rs/execpolicy2

### Overview

The `codex-execpolicy2` crate is the second-generation command policy evaluation engine. It provides a cleaner, more extensible approach to determining command safety using a rule-based system with explicit decision types.

### How it fits into the larger codebase

Execpolicy2 is used alongside or as a replacement for execpolicy:

- **Core** may use for command safety evaluation
- **Provides** clearer decision semantics than execpolicy
- **Supports** more complex policy rules

### Core Implementation

**Key Components:**

- `policy.rs`: Policy definition with rules
- `rule.rs`: Individual rule definitions
- `parser.rs`: Policy file parsing
- `decision.rs`: Evaluation decision types
- `error.rs`: Error handling

**Decision Types:**

```rust
pub enum Decision {
    Allow,
    Deny,
    Unknown,  // Requires further analysis
}
```

### Things to Know

**Rule Structure:**

Rules in `rule.rs` define:
- Pattern matching for commands
- Argument constraints
- Decision outcome

**Policy Evaluation:**

Returns explicit `Decision` enum rather than boolean, allowing callers to handle unknown cases differently.

**Standalone Usage:**

`main.rs` provides a CLI for testing policy evaluation against commands.

Created and maintained by Nori.
