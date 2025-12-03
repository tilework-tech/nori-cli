# Noridoc: keyring-store

Path: @/codex-rs/keyring-store

### Overview

The `codex-keyring-store` crate provides system keychain integration for secure credential storage. It wraps the `keyring` crate to store sensitive data like API keys and tokens in the OS credential store.

### How it fits into the larger codebase

Keyring store is used by core for secure credential storage:

- **Core** auth module uses for token storage
- **Provides** secure alternative to plaintext auth.json
- **Supports** macOS Keychain, Windows Credential Manager, Linux Secret Service

### Core Implementation

Wraps `keyring` crate with:
- Codex-specific service name
- Error handling
- Cross-platform abstraction

### Things to Know

**Platform Support:**

- macOS: Keychain Access
- Windows: Credential Manager
- Linux: Secret Service (GNOME Keyring, KWallet)

**Fallback:**

When keyring is unavailable, falls back to file-based storage in `~/.codex/auth.json`.

Created and maintained by Nori.
