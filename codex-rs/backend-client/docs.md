# Noridoc: backend-client

Path: @/codex-rs/backend-client

### Overview

The `codex-backend-client` crate provides HTTP client utilities for communicating with the OpenAI backend and related services. It handles authentication, request signing, and common API patterns.

### How it fits into the larger codebase

Backend client is used by core and other crates for API communication:

- **Core** chat completions and responses API
- **ChatGPT** crate for cloud task operations
- **Handles** both API key and OAuth token auth

### Core Implementation

**Key Files:**

- `client.rs`: HTTP client wrapper with auth handling
- `types.rs`: Request/response type definitions
- `lib.rs`: Public exports

### Things to Know

**Authentication:**

Supports both:
- Bearer token auth (OAuth tokens from ChatGPT login)
- API key auth (direct OpenAI API key)

**Request Patterns:**

Provides utilities for:
- SSE streaming responses
- JSON request/response
- Error handling and retries

Created and maintained by Nori.
