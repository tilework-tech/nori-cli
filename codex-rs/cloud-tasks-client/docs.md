# Noridoc: cloud-tasks-client

Path: @/codex-rs/cloud-tasks-client

### Overview

The `codex-cloud-tasks-client` crate provides an HTTP client for the Codex Cloud Tasks API. It handles communication with the backend for listing, fetching, and managing cloud-based coding tasks.

### How it fits into the larger codebase

Cloud tasks client is used by:

- **cloud-tasks** TUI for API communication
- **Handles** authentication and request formatting
- **Supports** mock server for testing

### Core Implementation

**Key Files:**

- `api.rs`: API endpoint definitions and response types
- `http.rs`: HTTP client implementation
- `mock.rs`: Mock server for testing

### Things to Know

**API Operations:**

- List tasks with pagination
- Fetch task details
- Retrieve diffs for application

**Mock Support:**

`mock.rs` provides a fake server for unit and integration testing without real API calls.

Created and maintained by Nori.
