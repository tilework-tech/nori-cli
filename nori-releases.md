# Nori Releases

## Upstream Release Cadence

Upstream releases are very rapid (multiple releases per week):

- rust-v0.58.0: Nov 13
- rust-v0.59.0: Nov 19
- rust-v0.60.1: Nov 19 (same day!)
- rust-v0.61.0: Nov 20
- rust-v0.62.0: Nov 21
- rust-v0.63.0: Nov 21 (same day!)

Release Workflow (from rust-release.yml):

1. Manual process: git tag -a rust-vX.Y.Z → git push origin rust-vX.Y.Z
2. CI validates tag matches codex-rs/Cargo.toml version
3. Builds multi-platform binaries with code signing
4. Publishes to GitHub Releases and npm

## Branching Strategy

```
upstream/main ──●──●──●──●──●──●──●──●──●──●──●──●──●──●──●──●───→
                │        ▲              ▲              ▲
                │        │0.61.0        │0.63.0        │future release
                │        │              │              │
                ▼        │              │              │
fork/upstream-sync ──────┴──────────────┴──────────────┴───────→
                │                       │
                │ merge                 │ merge
                ▼                       ▼
origin/main ────●────●────●────●────────●────●────●────────────→ (your ACP work)
```

Branch Roles:

| Branch             | Purpose                                       |
| ------------------ | --------------------------------------------- |
| origin/main        | Active development and releases                |
| fork/upstream-main | Tracks upstream/main exactly (already exists) |
| fork/upstream-sync | Sync point branch for merges                  |

## Automated Sync (CI)

The `upstream-sync` GitHub Actions workflow automatically detects new stable
upstream releases and creates draft PRs.

**Trigger:** Daily at 9 AM UTC (scheduled) or manual via workflow_dispatch

**What it does:**

1. Fetches upstream tags
2. Finds latest stable tag (X.Y.Z only, no alpha/beta)
3. Updates `fork/upstream-sync` branch to point to the tag
4. Creates `sync/upstream-vX.Y.Z` branch from the tag
5. Opens a draft PR against `main` with merge instructions

**Manual trigger:**

```bash
# Sync latest stable release
gh workflow run upstream-sync.yml

# Sync specific tag
gh workflow run upstream-sync.yml -f tag=rust-v0.63.0

# Dry run (test without creating branches/PRs)
gh workflow run upstream-sync.yml -f dry_run=true
```

**Idempotency:** If a sync branch already exists, the workflow skips that release.

## Manual Sync Workflow

For manual syncing (or if CI is unavailable):

1. Update tracking branch

```bash
git fetch upstream --tags
git branch -f fork/upstream-sync rust-v0.63.0
git push origin fork/upstream-sync --force
```

2. Create sync branch from the release tag

```bash
git checkout -b sync/upstream-v0.63.0 rust-v0.63.0
git push origin sync/upstream-v0.63.0
```

3. Merge into main with conflict resolution

```bash
git checkout main
git merge sync/upstream-v0.63.0 --no-ff -m "Sync upstream rust-v0.63.0"
```

4. Resolve conflicts, test, push

```bash
cd codex-rs && cargo test
cargo insta review  # if snapshot tests need updating
git push origin main
```

## Downstream Nori Releases

We maintain our own separate versioning scheme (`nori-vX.Y.Z`) to avoid blocking
on upstream releases for our release tagging.

### How It Works: Synthetic Commits

Nori uses "synthetic commits" for releases, similar to upstream OpenAI/Codex:

```
main branch: ──●──●──●──●──●──●──●──  (Cargo.toml = placeholder, e.g., "0.0.0")
                              │
                              │ script creates synthetic commit via GitHub API
                              ▼
              [synthetic commit with Cargo.toml = "0.2.0"]
                              │
                              ▼
                         nori-v0.2.0 (tag)
```

**Key benefits:**

- The `main` branch's `Cargo.toml` never needs manual version bumps
- Release tags point to immutable snapshots with the correct version
- No "version bump" PRs cluttering git history
- Version is derived from existing releases automatically

### Creating Releases

Use the `create_nori_release` script to create releases:

```bash
# Preview what would happen (recommended first)
./scripts/create_nori_release --dry-run --publish-release

# Create next stable release (auto-increments minor version)
./scripts/create_nori_release --publish-release

# Create next alpha release (for upcoming version, e.g., 0.3.0-alpha.1)
./scripts/create_nori_release --publish-alpha

# Create a snapshot for internal testing (e.g., 0.2.0-next.1)
./scripts/create_nori_release --publish-next

# Create a specific version
./scripts/create_nori_release --version 0.3.0
./scripts/create_nori_release --version 0.3.0-alpha.1

# Query current version info (useful for debugging)
./scripts/create_nori_release --get-latest-stable  # prints latest stable version
./scripts/create_nori_release --get-next-version   # prints next -next.N version
```

**Version Detection:** The script determines versions from git tags (not GitHub
Releases). This is robust against incomplete release workflows where a tag exists
but the GitHub Release was never created due to workflow cancellation or failure.

### Creating Snapshots (@next)

For internal testing before a stable release, use the `--publish-next` flag:

```bash
# Preview what version would be created
./scripts/create_nori_release --dry-run --publish-next

# Create and publish a snapshot
./scripts/create_nori_release --publish-next
```

This creates versions like `0.2.0-next.1`, `0.2.0-next.2`, etc., based on the latest
stable release. These are published to npm with the `next` tag.

**From GitHub UI (no local tooling required):**

```bash
# Publish a snapshot directly from GitHub Actions
gh workflow run nori-release.yml -f publish_next=true -f dry_run=false

# Preview what version would be created (dry run)
gh workflow run nori-release.yml -f publish_next=true
```

The script:

1. Determines the next version (or uses the one you specify)
2. Creates a synthetic commit via GitHub API with updated `Cargo.toml`
3. Creates an annotated tag pointing to that commit
4. Pushes the tag, which triggers the CI workflow

**Requirements:**

- GitHub CLI (`gh`) must be installed and authenticated
- You need push access to the repository

### What Happens After Tag Push

The `nori-release.yml` workflow automatically:

1. Validates the tag format
2. Runs tests
3. Builds native binaries for all 4 platforms
4. Publishes to npm as `nori-ai-cli`
5. Creates a GitHub Release with changelog and artifacts

### Testing the Build (Dry Run)

To test the build process without creating a release:

```bash
gh workflow run nori-release.yml -f version=0.2.0 -f dry_run=true
```

This builds everything but doesn't publish or create tags.

To skip tests during a dry run (for faster iteration on build issues):

```bash
gh workflow run nori-release.yml -f version=0.2.0 -f dry_run=true -f skip_tests=true
```

Note: Tests are always run for actual releases (tag pushes). The `skip_tests` flag only works with `workflow_dispatch` dry runs.

### npm Package

- **Package name:** `nori-ai-cli`
- **Stable releases:** Published with `latest` tag, and `next` tag is also updated to point to the stable version
- **Pre-releases:** Published with `next` tag only (e.g., `0.2.0-alpha.1`)

```bash
# Install stable version
npm install -g nori-ai-cli

# Install pre-release or latest stable (both work after a stable release)
npm install -g nori-ai-cli@next
```

### Required Secrets

| Secret      | Purpose                                 |
| ----------- | --------------------------------------- |
| `NPM_TOKEN` | npm authentication token for publishing |

### Build Targets

The workflow builds native binaries for:

- Linux x86_64 (`x86_64-unknown-linux-gnu`)
- Linux ARM64 (`aarch64-unknown-linux-gnu`)
- macOS x86_64 (`x86_64-apple-darwin`)
- macOS ARM64 (`aarch64-apple-darwin`)

### Version Numbering

The script automatically determines the next version:

| Current Latest  | `--publish-release` | `--publish-alpha` | `--publish-next` |
| --------------- | ------------------- | ----------------- | ---------------- |
| None            | `0.1.0`             | `0.1.0-alpha.1`   | `0.0.0-next.1`   |
| `0.1.0`         | `0.2.0`             | `0.2.0-alpha.1`   | `0.1.0-next.1`   |
| `0.2.0-alpha.3` | `0.3.0`             | `0.3.0-alpha.1`   | `0.2.0-next.1`\* |
| `0.2.0`         | `0.3.0`             | `0.3.0-alpha.1`   | `0.2.0-next.1`   |
| `0.2.0-next.5`  | `0.3.0`             | `0.3.0-alpha.1`   | `0.2.0-next.6`   |

\*Note: `-next` versions are always based on the latest **stable** release, not alphas.
