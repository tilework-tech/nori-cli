# Noridoc: codex-execpolicy

Path: @/nori-rs/execpolicy

### Overview

The execpolicy crate provides parsing and evaluation of execution policies for command approval. Policies define which commands can be auto-approved based on patterns for executables, arguments, and their combinations.

### How it fits into the larger codebase

Its only consumer is the hidden `nori execpolicy check` subcommand in `@/nori-rs/cli/src/main.rs`, which checks policy files against a command and prints the result as JSON. `codex-core` still lists the crate as a dependency but does not use it. No runtime approval path calls this crate.

### Core Implementation

**Policy format** (`parser.rs`): Policies are Starlark files that call `prefix_rule`:

```starlark
prefix_rule(
    pattern = ["git", ["status", "log", "diff"]],  # ordered tokens; a list is a set of alternatives
    decision = "allow",                            # allow | prompt | forbidden; defaults to allow
    match = ["git status"],                        # examples that must match
    not_match = ["git push"],                      # examples that must not match
)
```

`PolicyParser::parse` evaluates one file and adds its rules; call it once per file, then `build()` returns a `Policy`. The `match` and `not_match` examples are checked at load time, so a wrong rule fails to load.

**Rules** (`rule.rs`): A `PrefixRule` holds a `PrefixPattern` (first token plus the remaining `PatternToken`s) and a `Decision`. A rule matches when the command starts with the pattern.

**Evaluation** (`policy.rs`): `Policy::check(cmd)` looks up rules by the command's first token and returns `Evaluation::Match` with every matching rule, or `Evaluation::NoMatch`. The effective decision is the strictest match (`forbidden` > `prompt` > `allow`, see `decision.rs`). `Policy::check_multiple` merges matches across several commands.

**CLI** (`execpolicycheck.rs`, `main.rs`): `ExecPolicyCheckCommand` takes repeatable `--policy` paths, an optional `--pretty`, and the command tokens. It is shared by `nori execpolicy check` and the standalone `codex-execpolicy` dev binary.

### Things to Know

- No policies are embedded; every policy file is passed with `--policy`. `examples/example.codexpolicy` is a sample.
- Evaluation is per command; the crate keeps no session state.
- Tests live in `tests/basic.rs`. `README.md` documents the JSON output shapes.

Created and maintained by Nori.
