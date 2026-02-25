# Noridoc: Scripts

Path: @/scripts

### Overview

- Utility scripts for release management, development setup, and code quality checks
- The most critical script is `create_nori_release`, which creates tagged releases via the GitHub API using synthetic commits

### How it fits into the larger codebase

- `create_nori_release` is the authoritative version-numbering tool -- it is called both directly by developers (`--publish-release`, `--publish-alpha`, `--publish-next`) and by the CI workflow `@/.github/workflows/nori-release.yml` (via `--get-next-version`) to determine `@next` snapshot versions
- The script creates synthetic commits via the GitHub API that modify `@/codex-rs/Cargo.toml` with the release version, then tags those commits; this keeps the `main` branch's `Cargo.toml` at `0.0.0` permanently
- The release tags created by this script are what trigger the `nori-release.yml` workflow's tag-push code path

### Core Implementation

**`create_nori_release` -- release creation via synthetic commits:**

The script uses the GitHub API exclusively (via `gh api`) rather than local git operations. The release flow is:

```
get_branch_head() -> fetch Cargo.toml -> replace_version() -> create_blob() -> create_tree() -> create_commit() -> create_tag() -> create_tag_ref()
```

All tags use the prefix `nori-v` (e.g., `nori-v0.9.0`, `nori-v0.9.0-next.3`).

**Version determination (`determine_version()`):**

| Mode | Base version | Suffix pattern | Example |
|---|---|---|---|
| `--publish-release` | latest stable + minor bump | none | `0.10.0` |
| `--publish-next` | latest stable (as-is) | `-next.N` | `0.9.0-next.3` |
| `--publish-alpha` | latest stable + minor bump | `-alpha.N` | `0.10.0-alpha.2` |

The `N` suffix is determined by scanning all git tags (via `list_tags()`) that match the relevant prefix and taking `max(N) + 1`. The `list_tags()` function paginates through the GitHub refs/tags API and strips the `nori-v` prefix from each tag to yield bare version strings.

`get_latest_release_version()` also uses `list_tags()` -- it filters to stable-only versions (no `-` in the version string) and returns the highest by semver comparison.

### Things to Know

- Git tags are the single source of truth for version enumeration -- `list_tags()` queries the GitHub refs/tags API, not GitHub Releases; this matters because synthetic-commit tags may not always have corresponding GitHub Releases (e.g., if the release workflow was cancelled after tagging)
- The `--get-next-version` flag simulates `--publish-next` internally to compute the version, then prints it and exits without creating any tags; this is how the CI workflow determines what version to build
- The version determination logic in `determine_version()` is noted in comments as being "mirrored" in `@/.github/workflows/nori-release.yml` -- changes to version numbering logic must be kept in sync between the two

Created and maintained by Nori.
