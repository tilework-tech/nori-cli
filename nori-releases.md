# Nori Releases

## Upstream Release Cadence

Upstream releases are very rapid (multiple releases per week):

- rust-v0.58.0: Nov 13
- rust-v0.59.0: Nov 19
- rust-v0.60.1: Nov 19  (same day!)
- rust-v0.61.0: Nov 20
- rust-v0.62.0: Nov 21
- rust-v0.63.0: Nov 21  (same day!)

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
origin/dev ─────●────●────●────●────────●────●────●────────────→ (your ACP work)
```

Branch Roles:

| Branch             | Purpose                                      |
|--------------------|-----------------------------------------------|
| origin/main        | Stable releases of your fork                  |
| origin/dev         | Active development (ACP features)             |
| fork/upstream-main | Tracks upstream/main exactly (already exists) |
| fork/upstream-sync | NEW: Sync point branch for merges             |

## Automated Sync (CI)

The `upstream-sync` GitHub Actions workflow automatically detects new stable
upstream releases and creates draft PRs.

**Trigger:** Daily at 9 AM UTC (scheduled) or manual via workflow_dispatch

**What it does:**

1. Fetches upstream tags
2. Finds latest stable tag (X.Y.Z only, no alpha/beta)
3. Updates `fork/upstream-sync` branch to point to the tag
4. Creates `sync/upstream-vX.Y.Z` branch from the tag
5. Opens a draft PR against `dev` with merge instructions

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

3. Merge into dev with conflict resolution
```bash
git checkout dev
git merge sync/upstream-v0.63.0 --no-ff -m "Sync upstream rust-v0.63.0"
```

4. Resolve conflicts, test, push
```bash
cd codex-rs && cargo test
cargo insta review  # if snapshot tests need updating
git push origin dev
```

## Downstream Nori Releases

We maintain our own separate versioning scheme (`nori-vX.Y.Z`) to avoid blocking
on upstream releases for our release tagging.

### Automated Release Workflow (Recommended)

The `nori-release.yml` GitHub Actions workflow automates the entire release process:

1. **Validates** the version format and ensures the tag doesn't exist
2. **Runs tests** on the dev branch (optional, can be skipped)
3. **Creates a git tag** `nori-vX.Y.Z` on the dev branch
4. **Builds native binaries** for all platforms:
   - Linux x86_64
   - Linux ARM64
   - macOS x86_64
   - macOS ARM64
5. **Publishes to npm** as `nori-ai-cli`
6. **Creates a GitHub Release** with changelog and binary artifacts

#### Usage

```bash
# Dry run first (recommended) - builds everything but doesn't publish
gh workflow run nori-release.yml -f version=0.2.0 -f dry_run=true

# Actual release
gh workflow run nori-release.yml -f version=0.2.0

# Skip tests if needed (use with caution)
gh workflow run nori-release.yml -f version=0.2.0 -f skip_tests=true
```

#### Workflow Inputs

| Input | Required | Default | Description |
|-------|----------|---------|-------------|
| `version` | Yes | - | Release version (e.g., `0.2.0` or `0.2.0-alpha.1`) |
| `dry_run` | No | `false` | Build and stage without publishing |
| `skip_tests` | No | `false` | Skip running tests before release |

#### npm Package

- **Package name:** `nori-ai-cli`
- **Stable releases:** Published with `latest` tag
- **Pre-releases:** Published with `next` tag (e.g., `0.2.0-alpha.1`)

```bash
# Install stable version
npm install -g nori-ai-cli

# Install pre-release
npm install -g nori-ai-cli@next
```

#### Required Secrets

| Secret | Purpose |
|--------|---------|
| `NPM_TOKEN` | npm authentication token for publishing |
| `PAT_NORI_RELEASE` | GitHub PAT for pushing tags (optional, falls back to GITHUB_TOKEN) |

### Manual Release (Legacy)

For manual releases without the workflow:

```bash
git checkout dev
git tag -a nori-v0.2.0 -m "Nori release 0.2.0"
git push origin nori-v0.2.0
```

Note: Manual releases will not automatically publish to npm or create GitHub releases.

