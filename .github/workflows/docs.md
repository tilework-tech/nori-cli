# Noridoc: GitHub Workflows

Path: @/.github/workflows

### Overview

- Contains GitHub Actions CI/CD workflows for the Nori CLI project
- Handles continuous integration, release publishing, and upstream synchronization
- All Rust builds require sccache due to workspace-level configuration

### How it fits into the larger codebase

- Triggered by git events (push, tag, PR) and manual workflow dispatches
- Publishes npm packages to npmjs.com for distribution via `nori-ai-cli`
- Integrates with @/codex-rs workspace which mandates sccache in `.cargo/config.toml`
- Uses Blacksmith ARM64 runners for faster native Linux builds
- Syncs changes from upstream Cline repository via automated PR workflow
- Build artifacts (binaries) are used by @/codex-cli npm wrapper for cross-platform distribution

### Core Implementation

**Workflow Files:**
- `nori-release.yml` - Builds and publishes nori-ai-cli releases to npm. Triggered by `nori-v*.*.*` tags created via `@/scripts/create_nori_release`
- `rust-ci.yml` - Runs tests, clippy, and builds on PRs and commits. Matrix strategy across Linux/macOS/musl targets
- `upstream-sync.yml` - Automatically syncs changes from upstream Cline fork, sanitizes workflows to prevent recursive triggers
- `cargo-deny.yml` - Validates dependencies for security and licensing issues

**Caching Architecture:**
All Rust workflows use a two-tier caching strategy:

1. **Cargo Home Cache** - Caches registry, git dependencies, and cargo binaries
   - Key: `cargo-home-{workflow}-{target}-{lockfile-hash}-{toolchain-hash}`
   - Significantly reduces download times for dependencies

2. **sccache Compilation Cache** - Caches compiled Rust artifacts across builds
   - Primary: GitHub Actions cache backend (when available via OIDC)
   - Fallback: Local filesystem cache with `actions/cache`
   - Key: `sccache-{workflow}-{target}-{lockfile-hash}-{run-id}`
   - Version: 0.7.5 (installed via `taiki-e/install-action`)

**Why sccache is Required:**
On January 8, 2026, commit `7cbd16f5c` added `rustc-wrapper = "sccache"` to `@/codex-rs/.cargo/config.toml`, making sccache mandatory for all workspace builds. This dramatically reduces disk usage and build times when using git worktrees, as compilation artifacts are shared across all worktrees.

**sccache Backend Detection:**
Workflows auto-detect the best caching backend:
```bash
if [[ -n "${ACTIONS_CACHE_URL:-}" && -n "${ACTIONS_RUNTIME_TOKEN:-}" ]]; then
  # Use GitHub Actions cache backend (preferred)
  SCCACHE_GHA_ENABLED=true
else
  # Fallback to local disk + actions/cache
  SCCACHE_DIR=${{ github.workspace }}/.sccache
fi
```

**Release Process:**
The `nori-release.yml` workflow uses "synthetic commits" - tags point to commits that exist only for the release (not on any branch), with `Cargo.toml` versions updated. This keeps the dev branch at a placeholder version while allowing proper semantic versioning for releases.

### Things to Know

- **Cache Key Namespacing**: Release workflow uses "release" prefix in cache keys to avoid conflicts with CI workflow caches
- **Musl Targets**: Linux builds target `x86_64-unknown-linux-musl` for maximum glibc compatibility across distributions
- **ARM64 Runners**: Uses Blacksmith-provided ARM64 runners for native Linux builds (faster than cross-compilation)
- **sccache Statistics**: All workflows display compilation cache hit rates in job summaries for monitoring effectiveness
- **Workflow Sandboxing**: Network operations (`git push`, `gh` CLI) must run with `dangerouslyDisableSandbox: true` as the GitHub Actions runner restricts network access in sandboxed mode
- **Critical Dependency**: If sccache is not installed in a workflow but the workspace requires it, all Rust builds will fail with "No such file or directory (os error 2)" when Cargo tries to invoke the sccache wrapper
- **Cache Restore Keys**: Use hierarchical restore keys to maximize cache hits across different lockfile versions and run IDs

Created and maintained by Nori.
