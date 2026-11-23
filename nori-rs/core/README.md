# codex-core

Inherited from the Codex fork. It now holds authentication and compatibility code used behind `codex-login`; it is not the Nori config boundary and runs no sessions. See [docs.md](docs.md).

## Dependencies

Core itself runs no sandboxed commands. Sandboxed execution lives in `codex-sandbox`, which expects these helpers from the `nori` binary. The `nori sandbox` subcommands use the same paths.

### macOS

Expects `/usr/bin/sandbox-exec` to be present (`nori sandbox macos`, alias `seatbelt`).

### Linux

Expects the binary to run the Landlock and seccomp helper when `arg0` is `codex-linux-sandbox` (`nori sandbox linux`, alias `landlock`). See the `codex-arg0` crate for details.

### All Platforms

Expects the binary to simulate the virtual `apply_patch` CLI when `arg1` is `--codex-run-as-apply-patch`. See the `codex-arg0` crate for details.
