# Releasing the Nori CLI

This runbook cuts stable and snapshot releases, tests the build without
publishing, and recovers a failed publish. Version numbers, tags, dist-tags,
and assets are defined in [../release-artifacts.md](../release-artifacts.md).

## How a release runs

The workflow `.github/workflows/nori-release.yml` runs these jobs in order:

```text
validate -> test -> build-native (4 targets) -> stage-npm
  -> create-next-tag (snapshots only) -> publish-npm -> create-release
                                      \-> dry-run-summary (dry runs only)
```

| Job | What it does |
|---|---|
| `validate` | Picks the version and tag, and the mode: release, snapshot, or dry run. |
| `test` | `cargo check --profile ci-test --target x86_64-unknown-linux-musl --all-targets`. Skipped when a dispatch sets `skip_tests`. |
| `build-native` | Builds `nori` for each target. |
| `stage-npm` | Packs `nori-ai-cli-<version>.tgz`. |
| `create-next-tag` | Snapshots only: creates the synthetic commit and the `nori-v<version>` tag. |
| `publish-npm` | Fails if the version is already on npm. Publishes with `--tag latest` or `--tag next`. For a stable version, then moves `next` to it. |
| `create-release` | Creates the GitHub release with the archives and the npm tarball. |

Triggers:

| Trigger | Mode |
|---|---|
| Push of a `nori-v*.*.*` tag | Release of that version |
| Push to `main` that touches `nori-rs/`, `nori-cli/`, `scripts/`, or the workflow | Snapshot (`@next`) |
| Manual dispatch with `publish_next=true` | Snapshot; a dry run unless `dry_run=false` |
| Manual dispatch with `version=X.Y.Z` | Dry run only |

A newer push to `main` cancels a snapshot run that is still in progress.

## Before you start

- Install `gh` and sign in with write access to `tilework-tech/nori-cli`.
- Run the script from a clone whose `origin` is `tilework-tech/nori-cli`.
  The script lists tags with `git ls-remote` against `origin`.
- Python 3 runs the script. It has no other dependencies.
- The script tags the **current head of `main` on GitHub**, not your local
  checkout. Make sure the commit you want is on `main` and its CI is green.

## Cut a stable release

1. Preview the version:

   ```bash
   ./scripts/create_nori_release --dry-run --publish-release
   ```

   It prints the version and tag and changes nothing. `--publish-release`
   bumps the minor version of the latest stable tag (`0.30.0` becomes
   `0.31.0`).

2. Create the tag:

   ```bash
   ./scripts/create_nori_release --publish-release
   ```

   For any other version, use `--version 0.30.1`. The script refuses a tag
   that already exists.

3. Watch the run:

   ```bash
   gh run list -R tilework-tech/nori-cli --workflow nori-release.yml --limit 3
   gh run watch -R tilework-tech/nori-cli <run-id>
   ```

4. Check the result:

   ```bash
   npm view nori-ai-cli dist-tags      # latest and next show the new version
   gh release view nori-v<version> -R tilework-tech/nori-cli
   ```

   The release must list four `nori-<version>-<platform>.tar.gz` archives and
   `nori-ai-cli-<version>.tgz`.

## Cut a snapshot (`@next`)

Merging to `main` publishes a snapshot by itself. To publish one without a new
commit:

```bash
gh workflow run nori-release.yml -R tilework-tech/nori-cli \
  -f publish_next=true -f dry_run=false
```

Or run `./scripts/create_nori_release --publish-next`. It creates the tag
`nori-vX.Y.Z-next.N`, and the tag push runs the release.

Print the next snapshot version without creating anything:

```bash
./scripts/create_nori_release --get-next-version
```

## Cut an alpha

```bash
./scripts/create_nori_release --dry-run --publish-alpha
./scripts/create_nori_release --publish-alpha
```

An alpha is a prerelease. It is published to the `next` dist-tag, like a
snapshot.

## Dry runs

A dry run builds every target and packs the npm package. It creates no tag,
publishes nothing, and creates no release. It ends with a summary of what a
real run would create.

```bash
# Build a given version from main
gh workflow run nori-release.yml -R tilework-tech/nori-cli -f version=0.31.0

# Faster: skip the test job
gh workflow run nori-release.yml -R tilework-tech/nori-cli \
  -f version=0.31.0 -f skip_tests=true

# Dry-run a snapshot
gh workflow run nori-release.yml -R tilework-tech/nori-cli -f publish_next=true
```

`dry_run` defaults to `true` for manual dispatch. A tag push or a push to
`main` always publishes.

Test the release script itself:

```bash
python3 scripts/test_create_nori_release.py
python3 scripts/test_create_nori_release_compatibility.py
```

## Recover a failed publish

First find the failed job with `gh run view <run-id> -R tilework-tech/nori-cli`.
Never delete or move a tag, release, or npm version that was published. See
[What never changes](../release-artifacts.md#what-never-changes).

| Failed job | State | Action |
|---|---|---|
| `validate`, `test`, `build-native`, or `stage-npm` on a tag push | Tag exists. Nothing published. | Re-run the failed jobs: `gh run rerun <run-id> --failed`. If the source is broken, fix it on `main` and cut a new version with `--version`. The failed tag still counts as the latest stable for numbering. |
| Any job before `publish-npm` on a snapshot | Maybe a tag, nothing published. | Do nothing. The next push to `main` publishes the next snapshot. The skipped `N` stays unused. |
| `publish-npm` | Tag exists. npm may or may not have the version. | Check `npm view nori-ai-cli@<version> version`. If it is missing, re-run the failed jobs. If it is present, do not re-run `publish-npm`; it fails on a published version. Re-run only `create-release` (see below). |
| `publish-npm`, step "Update @next tag" | npm `latest` is set; `next` is not moved. | The step may fail without failing the job. Run `npm dist-tag add nori-ai-cli@<version> next` with an npm token that can publish the package. |
| `create-release` | npm has the version. No GitHub release, or one with missing assets. | Re-run the failed jobs: `gh run rerun <run-id> --failed`. This reruns `create-release` only. Build artifacts are kept for 7 days. After that, create the release by hand from a dry-run build of the same tag, or cut a new version. |

Rules for recovery:

- Re-run the original run. A new run of the same tag fails at `publish-npm`
  once npm has the version.
- Never reuse a version number. When in doubt, cut the next version.
- When a consumer already pinned a version, the version's assets must stay
  exactly as they are.

## After a release

Consumers pin versions by hand. Bump them when needed:
`nori-sessions: docs/runbooks/bumping-pinned-runtimes.md`.
