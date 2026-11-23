# codex-linux-sandbox

This crate is responsible for producing:

- a `codex-linux-sandbox` standalone executable for Linux
- a lib crate that exposes the business logic of the executable as `run_main()` so that
  - the `nori` binary can check if its arg0 is `codex-linux-sandbox` and, if so, execute as if it were `codex-linux-sandbox` (see `codex-arg0`)
  - the sandbox exec engine (`codex-sandbox`) re-invokes `nori` under that arg0 to run a command under Landlock and seccomp
