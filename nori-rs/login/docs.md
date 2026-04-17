# Noridoc: codex-login

Path: @/nori-rs/login

### Overview

The login crate handles OAuth authentication flows for Nori. It supports device code authentication and runs a local HTTP server to receive OAuth callbacks.

### How it fits into the larger codebase

Used by `@/nori-rs/tui/` (via the `login` feature) to implement the `/login` slash command. Re-exports auth types from `@/nori-rs/core/` for convenience.

### Core Implementation

**Device Code Auth** (`device_code_auth.rs`): Implements OAuth device code flow:
1. Request device code from provider
2. Display user code and verification URL
3. Poll for token completion
4. Store token via auth manager

**PKCE** (`pkce.rs`): Implements Proof Key for Code Exchange for secure OAuth flows.

**Login Server** (`server.rs`): Runs a local HTTP server to:
- Serve the OAuth redirect endpoint
- Receive authorization codes
- Exchange codes for tokens

### Things to Know

- Re-exports `AuthMode`, `CodexAuth`, `AuthManager` from codex-core
- Supports both API key login and OAuth flows
- Tokens are stored in system keyring via `codex-keyring-store`
- The `CLIENT_ID` constant identifies Nori to OAuth providers

Created and maintained by Nori.
