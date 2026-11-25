# Release artifacts contract

This spec defines what a Nori CLI release publishes: version numbers, git
tags, npm dist-tags, GitHub release assets, and their layout. Other projects
download and pin these artifacts, so the names and layout are a contract.

Producer: `.github/workflows/nori-release.yml` and
`scripts/create_nori_release` in this repo.

Consumers:

| Consumer | What it downloads | How it pins |
|---|---|---|
| Nori Sessions machine bootstrap (`nori-sessions: broker/server/bootstrap/install-base.sh`) | `nori-<version>-linux-x86_64.tar.gz` from the GitHub release | `NORI_CLI_VERSION` plus the archive's SHA-256 |
| Nori Native (`nori-sessions: native/nori.lock.json`) | `nori-ai-cli-<version>.tgz` from the npm registry | npm `integrity` (SHA-512) plus each binary's SHA-256 |
| npm and Bun users | `nori-ai-cli` by dist-tag or version | The package manager |
| The TUI update check (`nori-rs/tui/src/nori/updates.rs`) | GitHub `releases/latest` | Reads the `nori-v` tag name |

How consumers bump their pins:
`nori-sessions: docs/runbooks/bumping-pinned-runtimes.md`. How to cut a
release: [runbooks/releasing.md](runbooks/releasing.md).

## Versions

A version is `X.Y.Z` or `X.Y.Z-<label>.N`, where `X`, `Y`, `Z`, and `N` are
integers and `<label>` is letters only. The workflow and the script reject any
other form.

| Kind | Form | Example | How the number is chosen |
|---|---|---|---|
| Stable | `X.Y.Z` | `0.30.0` | `--publish-release` bumps the minor of the latest stable tag. `--version` sets any version. |
| Snapshot | `X.Y.Z-next.N` | `0.30.0-next.6` | `X.Y.Z` is the latest stable tag. `N` is one more than the highest existing `X.Y.Z-next.*` tag. |
| Alpha | `X.Y.0-alpha.N` | `0.31.0-alpha.1` | `X.Y.0` is the next minor after the latest stable tag. `N` counts up from 1. |

Rules:

- Git tags are the source of truth for numbering. The script lists them with
  `git ls-remote --tags origin 'refs/tags/nori-v*'`. GitHub releases and npm
  are not consulted.
- "Latest stable" is the highest `X.Y.Z` tag without a `-` suffix. A stable
  tag counts even when its release workflow failed.
- A version with a `-` suffix is a prerelease.
- Numbers can have gaps. A snapshot whose run failed after its tag was created
  still uses up its `N`.
- The binary reports its version through `nori --version`. The version is
  written into `nori-rs/Cargo.toml` at release time. On `main` that field is
  the placeholder `0.0.0`.

## Git tags

- The tag name is `nori-v<version>`, for example `nori-v0.30.0` or
  `nori-v0.30.0-next.6`.
- The tag is an annotated tag with the message `Nori release <version>`.
- The tag points to a **synthetic commit**: a commit whose parent is the head of
  `main` and whose only change sets the first `version = "..."` line of
  `nori-rs/Cargo.toml`. Its message is `Release <version>`. The commit is on
  no branch.
- Stable and alpha tags are created by `scripts/create_nori_release`, which
  calls the GitHub API. Pushing the tag starts the workflow.
- Snapshot tags are created by the workflow itself, in the `create-next-tag`
  job.

## Channels and npm dist-tags

The npm package is `nori-ai-cli`.

| Release | npm dist-tag set at publish | GitHub release |
|---|---|---|
| Stable | `latest`. Then `next` is moved to the same version. | Normal release. Becomes GitHub's "latest". |
| Snapshot | `next` | Prerelease |
| Alpha | `next` | Prerelease |

- **`latest`** is the stable channel. The TUI update prompt installs
  `nori-ai-cli@latest`.
- **`next`** is the snapshot channel. Every push to `main` that touches
  `nori-rs/`, `nori-cli/`, `scripts/`, or the release workflow publishes a
  snapshot. Snapshots are a supported channel: consumers may pin a snapshot
  version. Pinning stable releases is the preferred direction.
- Dist-tags move. Versions do not. A consumer that needs a fixed build pins a
  version, never a dist-tag.
- The update check reads GitHub `releases/latest`, which skips prereleases.
  It never offers a snapshot.

## GitHub release

Each published version has one GitHub release on its tag.

- Name: `Nori <version>`.
- Prerelease flag: set when the version has a `-` suffix.
- Body: a changelog (from `git-cliff --latest` when the runner has it,
  otherwise `git log` since the previous tag), then
  `nori-rs/nori-config/data/harness-compat-release-note.md`, then the
  `npm install -g nori-ai-cli@<version>` line.

### Assets

| Asset | Contents |
|---|---|
| `nori-<version>-linux-x86_64.tar.gz` | Linux x86_64 binary |
| `nori-<version>-linux-arm64.tar.gz` | Linux arm64 binary |
| `nori-<version>-darwin-arm64.tar.gz` | macOS Apple silicon binary |
| `nori-<version>-darwin-x86_64.tar.gz` | macOS Intel binary |
| `nori-ai-cli-<version>.tgz` | The npm package, byte for byte the tarball published to npm |

No other assets are published. There is no checksum file and no signature
file.

### Archive layout

Each `nori-<version>-<platform>.tar.gz` is a gzip tar with exactly one file:

```text
./
./nori/
./nori/nori        the native binary
```

Consumers extract the archive and install `nori/nori` with mode `0755`. The
archive does not guarantee the execute bit.

### npm package layout

```text
package/
  package.json                 "version" set to the release version
  README.md                    the repo root README.md
  bin/nori.js                  launcher; the `nori` and `nori-ai-cli` commands
  bin/platform.js              maps Node's platform and arch to a target triple
  vendor/<target-triple>/nori/nori    one native binary per target (mode 0755)
```

`bin/nori.js` resolves `vendor/<target-triple>/nori/nori` for the running
platform and runs it with the same arguments. It adds
`vendor/<target-triple>/path/` to `PATH` when that folder exists. It sets
`NORI_MANAGED_BY_BUN=1` when Bun installed the package and
`NORI_MANAGED_BY_NPM=1` otherwise. It forwards `SIGINT`, `SIGTERM`, and
`SIGHUP` to the binary and exits with the binary's exit code or signal.

## Platform targets

| Rust target triple | Archive `<platform>` | npm `os` / `cpu` | Build runner |
|---|---|---|---|
| `x86_64-unknown-linux-musl` | `linux-x86_64` | `linux` / `x64` | Ubuntu 24.04 x86_64 |
| `aarch64-unknown-linux-musl` | `linux-arm64` | `linux` / `arm64` | Ubuntu 24.04 arm64 |
| `aarch64-apple-darwin` | `darwin-arm64` | `darwin` / `arm64` | macOS 15 |
| `x86_64-apple-darwin` | `darwin-x86_64` | `darwin` / `x64` | macOS 15 |

- The Linux binaries are statically linked against musl. They run on any
  Linux distribution of that architecture.
- The toolchain is Rust 1.90.0. The build is
  `cargo build --release --target <triple> -p nori-cli`.
- Windows and Android are not built.

## Integrity

- npm records a SHA-512 `integrity` for every version. The package is
  published with npm Trusted Publishing (OIDC), so npm attaches a provenance
  attestation that names the workflow run.
- GitHub reports a SHA-256 `digest` for each release asset through its API.
- Nori publishes no checksum file of its own. A consumer that pins an
  artifact computes or records its own SHA-256 and checks it on download.

## What never changes

Once a version is published (its npm version or its GitHub release exists):

- The tag `nori-v<version>` exists and points to the same commit.
- The GitHub release and every asset listed above stay, with the same bytes.
- The npm version stays, with the same bytes. npm forbids republishing a
  version.
- Asset names, the archive layout, the npm package layout, and the target
  triples keep the forms above. A change to any of them is a breaking change
  for consumers and is made only together with the consumers.

These rules hold for snapshots as well as stable releases.

What can change:

- The `latest` and `next` dist-tags, and GitHub's "latest release".
- The release body text.

## Where `main` differs

- Nothing enforces immutability. The repo has no tag ruleset, so a tag,
  release, or asset can still be deleted or replaced by hand.
- A snapshot's binaries and its tag can come from different commits. Each job
  checks out `main` again, and `create-next-tag` tags the head of `main` at
  the time it runs, not the commit that was built.
- The release archives store `nori/nori` without the execute bit, because
  workflow artifacts drop file modes and only the npm staging step restores
  them.
- The workflow header and `create_nori_release --help` still call snapshots
  "for testing" and "for internal testing".
- `nori-cli/package.json` lists `android` in `os` and `bin/platform.js` maps
  Android and Windows triples, but no Android or Windows binary is shipped. An
  Android install succeeds and then fails to start.
