# Noridoc: GitHub Workflows

Path: @/.github/workflows

### Overview

- CI and release automation for the nori-cli repository
- The primary workflow is `nori-release.yml`, which builds cross-platform native binaries and publishes the `nori-ai-cli` npm package
- Additional workflows handle Rust CI checks (`rust-ci.yml`) and dependency auditing (`cargo-deny.yml`)

### How it fits into the larger codebase

- The release workflow builds Rust binaries from `@/codex-rs/` and packages them via the Node.js launcher in `@/nori-cli/`
- Version detection delegates to `@/scripts/create_nori_release --get-next-version`, which queries git tags (via the GitHub API) as the single source of truth for version numbering
- Stable releases use "synthetic commits" created by the `create_nori_release` script -- release tags point to commits that exist only for the release (not on any branch), with `Cargo.toml` updated to the release version, keeping the `main` branch's `Cargo.toml` at a placeholder `0.0.0`
- The `nori-release.yml` workflow publishes to npm under the package name `nori-ai-cli`, with stable releases tagged `@latest` and snapshots tagged `@next`

### Core Implementation

**Release workflow trigger types:**

| Trigger | Condition | Code Path |
|---------|-----------|-----------|
| Tag push | `nori-v*.*.*` tag pushed (via `create_nori_release` script) | `is_tag_push=true` -- publishes a stable/alpha/beta release |
| Main branch push | Push to `main` (e.g., merged PR) with path filters | `publish_next=true` -- publishes a `@next` snapshot |
| Manual dispatch | `workflow_dispatch` with inputs | Either `publish_next=true` or explicit version + optional `dry_run` |

**Path filters for main branch pushes** restrict triggering to changes in `codex-rs/**`, `nori-cli/**`, `scripts/**`, and the workflow file itself, so docs-only changes do not trigger a release.

**Concurrency control** uses group `nori-release-${{ github.ref }}` with `cancel-in-progress` enabled only for main branch pushes. This means if two PRs merge in quick succession, the second run cancels the first since only the latest snapshot matters.

**Job pipeline:**

```
validate -> test -> build-native (matrix: 4 targets) -> stage-npm -> create-next-tag -> publish-npm -> create-release
                                                                   \-> dry-run-summary (dispatch only)
```

- `validate` -- determines version, release type, and outputs (`is_tag_push`, `publish_next`, `checkout_ref`, etc.) that downstream jobs use in their `if` conditionals
- `build-native` -- cross-compiles for Linux (x86_64/aarch64 musl) and macOS (x86_64/aarch64)
- `stage-npm` -- assembles the npm package with platform-specific optional dependencies
- `create-next-tag` -- creates a lightweight git tag for `@next` releases only (skipped for tag pushes)
- `publish-npm` -- publishes to npm with `--tag next` for pre-releases or `--tag latest` for stable
- `create-release` -- creates a GitHub Release with native binary assets

**Version injection into the compiled binary:** The `main` branch keeps `Cargo.toml` at `version = "0.0.0"` as a placeholder. Different release types inject the real version differently:

| Release type | How version reaches `Cargo.toml` before `cargo build` |
|---|---|
| Tag push (stable/alpha) | The tag points to a synthetic commit where `Cargo.toml` already has the correct version baked in |
| `@next` snapshot | The `build-native` job runs a `sed` step to inject the version from `validate` outputs into `Cargo.toml` before building |

**Job conditional pattern:** Downstream jobs that should run for both tag pushes and `@next` publishes use the pattern `needs.validate.outputs.is_tag_push == 'true' || (needs.validate.outputs.publish_next == 'true' && (github.event_name != 'workflow_dispatch' || inputs.dry_run == false))`. The `workflow_dispatch` guard is necessary because `inputs.dry_run` is undefined for push events.

### Things to Know

- The `validate` job distinguishes tag pushes from main branch pushes using `startsWith(github.ref, 'refs/tags/')` rather than just checking `github.event_name == 'push'`, since both tag pushes and main branch pushes have `event_name == 'push'`
- Main branch `@next` publishes reuse the exact same version detection, tag creation, npm publish, and GitHub Release creation code paths as manual `publish_next` dispatches
- The `dry_run` input only applies to `workflow_dispatch` -- tag pushes and main branch pushes always publish for real
- Build runners use Blacksmith (e.g., `blacksmith-4vcpu-ubuntu-2404`) for most jobs, with standard `ubuntu-24.04` for npm publish (which needs the `npm-publish` environment)
- Git tags are the source of truth for all version numbering -- both the `validate` job (via `create_nori_release --get-next-version`) and the `create_nori_release` script's `determine_version()` function use `list_tags()` to enumerate existing versions; GitHub Releases are not consulted for version counting

Created and maintained by Nori.
