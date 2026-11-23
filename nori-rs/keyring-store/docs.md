# Noridoc: codex-keyring-store

Path: @/nori-rs/keyring-store

### Overview

- A platform-agnostic abstraction over OS keyring services for storing credentials. It wraps the `keyring` crate behind a `KeyringStore` trait so callers can substitute in-memory or failing stores in tests.
- The `codex-` crate name is inherited fork residue; the crate is used by Nori code.

### How it fits into the larger codebase

- [`nori-auth-providers`](../auth-providers/docs.md) stores the saved-credential document under service `Nori Credentials` (account `credentials`) when `NORI_CREDENTIALS_STORE=keyring` is set; its default file backend does not use the keyring. The CLI ([`auth.rs`](../cli/src/auth.rs)), the TUI's `/api-key`, and harness reach it through that crate.
- `@/nori-rs/rmcp-client/` (`src/oauth.rs`) stores MCP OAuth tokens.
- `@/nori-rs/core/` (`src/auth/storage.rs`) stores inherited Codex auth.
- Callers take an `Arc<dyn KeyringStore>` and pass `DefaultKeyringStore` in production.

### Core Implementation

**KeyringStore trait**, keyed by `(service, account)`:
- `load` returns `None` when no entry exists
- `save` stores the value
- `delete` returns `false` when no entry exists

**DefaultKeyringStore**: production implementation over the platform keyring (macOS Keychain, Linux kernel keyutils/Secret Service via `linux-native-async-persistent`, Windows Credential Manager, Secret Service on FreeBSD/OpenBSD).

**MockKeyringStore**: in-memory implementation in the public `tests` module, with per-account `MockCredential` handles so tests can inspect values or inject errors. It keys entries by account only and ignores the service.

### Things to Know

- This crate's own `Cargo.toml` enables the per-target `keyring` backend features; some dependent crates enable the same features again.
- `CredentialStoreError` wraps the underlying keyring error.
- Every operation logs start and outcome at `trace` level with service and account (and the value length on save), never the value.
- The `tests` module is `pub` and not `#[cfg(test)]`-gated, so other crates' tests (for example `nori-auth-providers`) can import `MockKeyringStore`.

Created and maintained by Nori.
