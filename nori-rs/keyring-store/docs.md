# Noridoc: codex-keyring-store

Path: @/nori-rs/keyring-store

### Overview

The keyring-store crate provides a platform-agnostic abstraction over system keyring services for secure credential storage. It wraps the `keyring` crate with a trait-based interface that supports mocking for tests.

### How it fits into the larger codebase

Used by `@/nori-rs/core/` (`auth.rs`) to store and retrieve authentication tokens securely.

### Core Implementation

**KeyringStore Trait**: Defines three operations:
- `load(service, account)` - Retrieve credential, returns `None` if not found
- `save(service, account, value)` - Store credential
- `delete(service, account)` - Remove credential, returns `false` if not found

**DefaultKeyringStore**: Production implementation using platform keyring:
- macOS: Keychain
- Linux: Secret Service (or file fallback)
- Windows: Credential Manager

**MockKeyringStore** (in `tests` module): In-memory implementation for unit testing.

### Things to Know

- Platform-specific keyring backends are selected via Cargo features in dependent crates
- `CredentialStoreError` wraps underlying keyring errors
- All operations are traced at debug level for diagnostics
- The mock uses `Arc<MockCredential>` for thread-safe testing

Created and maintained by Nori.
