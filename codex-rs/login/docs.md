# Noridoc: login

Path: @/codex-rs/login

### Overview

The `codex-login` crate implements authentication flows for Codex, including OAuth-based ChatGPT login and API key management. It provides both browser-based and device code authentication methods, with a local server to handle OAuth callbacks.

### How it fits into the larger codebase

Login is used by CLI commands and TUI onboarding:

- **CLI** `codex login` uses `LoginServer` for browser OAuth
- **CLI** `codex login --device-auth` uses `run_device_code_login()`
- **TUI** onboarding screen integrates with `AuthManager`
- **Re-exports** auth types from `codex-core` for convenience

### Core Implementation

**Server-based Login:**

`server.rs` provides `LoginServer`:
1. Starts local HTTP server on random port
2. Opens browser to ChatGPT login URL with PKCE challenge
3. Receives OAuth callback with authorization code
4. Exchanges code for tokens
5. Stores tokens via `AuthManager`

**Device Code Login:**

`device_code_auth.rs` implements RFC 8628:
1. Requests device code from OAuth provider
2. Displays user code and verification URL
3. Polls for token completion
4. Stores tokens on success

**PKCE Implementation:**

`pkce.rs` generates code verifiers and challenges for OAuth security.

### Things to Know

**Re-exports:**

For convenience, the crate re-exports from `codex-core`:
- `AuthManager`, `CodexAuth`, `AuthDotJson`
- `TokenData`
- Auth constants (`CLIENT_ID`, env var names)
- `login_with_api_key()`, `logout()`, `save_auth()`

**Server Options:**

`ServerOptions` configures:
- Port (default: 0 for random)
- Host (default: localhost)
- Custom OAuth endpoints (experimental)

**Shutdown Handle:**

`ShutdownHandle` allows graceful server shutdown from async context.

**Auth Modes:**

`AuthMode` (from app-server-protocol) distinguishes:
- `ChatGPT`: OAuth-based login
- `ApiKey`: Direct API key

Created and maintained by Nori.
