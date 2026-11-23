# Noridoc: nori-skillsets

Path: @/nori-rs/skillsets

### Overview

- Blocking Rust client for the skillsets v2 CLI (`nori-skillsets` 0.2.x, source in the sibling `skillsets-v2` repo). It exposes the only skillset operations Nori supports for v2: list, layer, and unlayer.
- Every call shells out to the v2 CLI in JSON mode and decodes its `{"ok": true, "data": ...}` / `{"ok": false, "error": {code, message}}` envelope into typed results or a `SkillsetsError`.
- Exists because the TUI's default skillset flow calls v1 verbs (`list`, `install`, `switch --install-dir`, `list-active`) that v2 does not have. The TUI uses this crate only when `[features] skillsets_v2` is on (off by default, experimental).

### How it fits into the larger codebase

```text
nori-tui  skillset picker / footer / /switch-skillset   ([features] skillsets_v2 only)
   |  (tokio::spawn_blocking)
   v
nori-skillsets::SkillsetsCli  --->  nori-skillsets (v2 CLI)  -n <verb> ... --project <dir> --json
                              --->  git ls-remote (published release tags)
```

- The only consumer today is [nori-tui](../tui/docs.md), and only with `[features] skillsets_v2` (`NoriConfig.skillsets_v2`): the flow in [chatwidget/skillsets_v2.rs](../tui/src/chatwidget/skillsets_v2.rs), the picker in [skillset_picker_v2.rs](../tui/src/nori/skillset_picker_v2.rs), and the footer's active-skillset list in [system_info.rs](../tui/src/system_info.rs). With the flag off the TUI uses the v1 picker in [skillset_picker.rs](../tui/src/nori/skillset_picker.rs) and never calls this crate. Calls are blocking, so the TUI runs them on a blocking thread.
- It is deliberately not wired into the background host ([cli/src/host](../cli/src/host/docs.md)) yet. The public result types derive `serde::Serialize` with camelCase fields so the host can later expose them to ACP clients unchanged.
- It is independent of the `nori skillsets ...` passthrough in [cli/src/main.rs](../cli/src/main.rs), which resolves the CLI differently (see [cli docs](../cli/docs.md)).
- The caller chooses the `project` directory every call targets; this crate has no notion of sessions or worktrees.

### Core Implementation

- `SkillsetsCli::resolve()` picks how to launch the CLI: the `NORI_SKILLSETS_COMMAND` env var, split with shell rules so a launcher can supply a runner plus arguments (for example a bundled `bun x nori-skillsets@0.2.0`), else `nori-skillsets` found on `PATH`. `resolve_from` takes both inputs explicitly for tests.
- Every invocation is `<program> <prefix args> -n <verb> <args> --json` with stdin null. v2 requires the non-interactive `-n` flag before the subcommand.
- Every child process (the v2 CLI and git) runs with `GIT_TERMINAL_PROMPT=0` and, unless the user already set `GIT_SSH_COMMAND`, `GIT_SSH_COMMAND="ssh -o BatchMode=yes"`. The TUI owns the terminal, so a credential or SSH prompt would otherwise hang or corrupt it.
- `layered(project)` returns layer names in stack order from `status --project`. It never touches the network; the TUI footer uses it when the v2 flag is on.
- `layer(project, name)` / `unlayer(project, name)` run the matching v2 verb and return a `LayerChange`: the resulting stack plus human-readable notes, which fold in v2's refusals (paths it would not overwrite) and kept files.
- `list(project)` builds a `SkillsetList`: layered skillsets first in stack order, then the remaining published skillsets by name, each with a version (the layered version, or the latest published one). If published releases cannot be read, `list` still succeeds with only the layered entries and sets `SkillsetList.warning` to the reason.

**Listing published skillsets.** v2 has no "list available skillsets" command, so `list` merges `status` with the repository's release tags:

| Step | Source |
|------|--------|
| Repository | `status.data.repository` for the project, else `config get repository` (machine-wide); none means only layered entries are returned |
| Releases | `git ls-remote --tags --refs <remote> 'refs/tags/skillsets/*'`, tags shaped `skillsets/<name>@<semver>` |
| Latest version | Highest stable semver, else highest prerelease, matching what `layer` picks; unparseable tags are skipped |

A repository that is a local checkout is queried from inside itself (`git -C <checkout> ls-remote` against `origin`, else its first remote, else `.`) so the checkout's own credential and transport config applies, mirroring v2's git source selection. A URL repository is queried directly.

### Things to Know

- Error mapping in `SkillsetsError`:

| Variant | When |
|---------|------|
| `NotInstalled` | No override and no `nori-skillsets` on `PATH`; message points at the v2 install script |
| `InvalidOverride` | `NORI_SKILLSETS_COMMAND` is empty or not a valid shell command line |
| `NotV2` | Output is not a JSON envelope, or the error code is `UNKNOWN_COMMAND` |
| `UnexpectedResponse` | A valid v2 success envelope whose `data` does not match the expected shape |
| `InvalidName` | Empty name or a name starting with `-`, rejected before spawning so it cannot be read as a flag |
| `Command` | Any other v2 error envelope, carrying v2's code and message |
| `Git` | `git ls-remote` (or `git remote`) failed while listing releases. `list` does not return it; it folds the message into `SkillsetList.warning` |
| `Spawn` | The resolved program could not be started |

- v1 and v2 share the binary name `nori-skillsets`, so a v1 install on `PATH` resolves successfully and only fails at the first call, surfacing as `NotV2`. On npm the package name currently resolves to v1; v2 is installed with `curl -fsSL https://cli.noriskillsets.dev/install.sh | sh`.
- Known gap: `git ls-remote` runs without v2's own registrar credentials, so a registrar-hosted repository that needs them (or any unreachable remote) cannot list published releases. Listing degrades to the layered entries plus a `warning`; layering and unlayering still work because v2 fetches with its own credentials. The intended fix is an upstream `nori-skillsets list --json`.
- `list` and the release lookup can hit the network; `layered` and `status` do not.

Created and maintained by Nori.
