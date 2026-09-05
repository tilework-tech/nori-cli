# Noridoc: Scripts

Path: @/scripts

### Overview

- Utility scripts for release management, development setup, code quality checks, cloud E2E testing, and infrastructure validation
- The most critical script is `create_nori_release`, which creates tagged releases via the GitHub API using synthetic commits
- Cloud-related scripts handle both developer environment setup (`setup-cloud-dev.sh`, `cloud_agent_setup.sh`) and full-stack E2E validation (`cloud-e2e-test.sh`); `test_justfile.sh` validates the root justfile targets

### How it fits into the larger codebase

- `create_nori_release` is the authoritative version-numbering tool -- it is called both directly by developers (`--publish-release`, `--publish-alpha`, `--publish-next`) and by the CI workflow `@/.github/workflows/nori-release.yml` (via `--get-next-version`) to determine `@next` snapshot versions
- The script creates synthetic commits via the GitHub API that modify `@/nori-rs/Cargo.toml` with the release version, then tags those commits; this keeps the `main` branch's `Cargo.toml` at `0.0.0` permanently
- The release tags created by this script are what trigger the `nori-release.yml` workflow's tag-push code path
- `cloud-e2e-test.sh` validates the full `nori cloud` stack (CLI -> broker -> sprite -> ACP bridge -> Claude) using real infrastructure; it depends on an external `nori-sessions` repo containing the broker code and on Fly.io Sprites for remote compute

### Core Implementation

**`create_nori_release` -- release creation via synthetic commits:**

The script separates read-only version discovery from release creation: it enumerates matching tags from the `origin` remote through the git protocol, then uses the GitHub API (via `gh api`) to create the synthetic commit and annotated tag without mutating the local checkout. The creation flow is:

```
get_branch_head() -> fetch Cargo.toml -> replace_version() -> create_blob() -> create_tree() -> create_commit() -> create_tag() -> create_tag_ref()
```

All tags use the prefix `nori-v` (e.g., `nori-v0.9.0`, `nori-v0.9.0-next.3`).

**Version determination (`determine_version()`):**

| Mode                | Base version               | Suffix pattern | Example          |
| ------------------- | -------------------------- | -------------- | ---------------- |
| `--publish-release` | latest stable + minor bump | none           | `0.10.0`         |
| `--publish-next`    | latest stable (as-is)      | `-next.N`      | `0.9.0-next.3`   |
| `--publish-alpha`   | latest stable + minor bump | `-alpha.N`     | `0.10.0-alpha.2` |

The `N` suffix is determined by scanning all git tags (via `list_tags()`) that match the relevant prefix and taking `max(N) + 1`. The tag listing uses `git ls-remote` against `origin` so all matching refs arrive in one git-protocol request; it strips the `nori-v` prefix and ignores the peeled refs emitted for annotated tags.

`get_latest_release_version()` also uses `list_tags()` -- it filters to stable-only versions (no `-` in the version string) and returns the highest by semver comparison.

**`cloud-e2e-test.sh` -- full-stack cloud session E2E test:**

Validates the complete `nori cloud` session lifecycle against real infrastructure. The test flow is:

```
check_prereqs -> build_nori -> patch_broker -> start_broker -> verify_auth
-> prepare_sprite -> wait_for_sprite_ready -> run_e2e (send message, verify response)
-> close_session -> wait_for_sprite_available -> run_reacquire_test (second session)
```

The script temporarily patches the broker to prevent it from deleting or refreshing sprites during the test (reverted on cleanup). It uses tmux to drive the `nori cloud` TUI and asserts on screen output. The test exercises the session lifecycle twice: acquire a session, send a message, close, re-acquire a fresh session, and send another message.

| Env var                  | Purpose                                         | Default                                                           |
| ------------------------ | ----------------------------------------------- | ----------------------------------------------------------------- |
| `NORI_SPRITE_TOKEN`      | Sprites API token (required)                    | --                                                                |
| `NORI_SPRITE_ORG`        | Sprites org name                                | `amol-kapoor`                                                     |
| `NORI_SESSIONS_WORKTREE` | Path to broker worktree in `nori-sessions` repo | `~/code/nori/nori-sessions/.worktrees/cli-cloud-session-refactor` |
| `NORI_BINARY`            | Path to nori binary                             | `nori-rs/target/debug/nori`                                       |
| `BROKER_PORT`            | Local broker port                               | `19400`                                                           |
| `SKIP_BROKER_START`      | Use an already-running broker                   | `0`                                                               |
| `SKIP_BUILD`             | Skip `cargo build --bin nori`                   | `0`                                                               |

### Shared Local Runner Layer Support

**`test_justfile.sh` -- integration tests for the root justfile:**

Validates the standard targets (`help`, `dev`, `test`, `doctor`) defined by the Shared Local Runner Layer spec in `@/justfile`. The test uses string-matching assertions (`assert_contains`) against command output and `just --summary` / `just --show` for structural checks. Run with `bash scripts/test_justfile.sh`.

### Things to Know

- Git tags are the single source of truth for version enumeration -- `list_tags()` reads the repository's `origin` remote rather than GitHub Releases, so the caller must run in a checkout with an accessible `origin`; this also preserves versions whose tag exists even if a cancelled or failed workflow never created the corresponding GitHub Release
- The `--get-next-version` flag simulates `--publish-next` internally to compute the version, then prints it and exits without creating any tags; this is how the CI workflow determines what version to build
- The version determination logic in `determine_version()` is noted in comments as being "mirrored" in `@/.github/workflows/nori-release.yml` -- changes to version numbering logic must be kept in sync between the two
- `cloud-e2e-test.sh` requires external infrastructure (Fly.io Sprites, Firebase auth) and a checkout of the `nori-sessions` broker repo -- it cannot run in CI or sandboxed environments; it patches broker source files in-place and relies on a cleanup trap to restore them
- The cloud E2E test uses `bun` to run TypeScript helper scripts against the Sprites API (via `@fly/sprites` SDK installed in the broker's `node_modules`), so the broker worktree must have its dependencies installed

Created and maintained by Nori.
