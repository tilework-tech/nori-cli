# Noridoc: codex-api

Path: @/codex-rs/codex-api

### Overview

The codex-api crate provides high-level API clients for AI provider APIs. It wraps the low-level transport from `codex-client` with typed request builders and response handling for the OpenAI Responses API endpoint.

### How it fits into the larger codebase

Used by `@/codex-rs/core/` for the legacy HTTP backend (non-ACP mode). Provides the Responses API client for OpenAI-compatible providers.

### Core Implementation

**Provider Abstraction** (`provider.rs`):
- `Provider` - Configures API endpoint, auth, and retry behavior. Always uses the OpenAI Responses API wire protocol; there is no wire format selector at this layer.

**Responses Client** (`endpoint/responses.rs`):
- `ResponsesClient` - OpenAI Responses API client
- `ResponsesOptions` - Configuration options

**Stream Aggregation** (`endpoint/aggregate.rs`):
- `AggregateStreamExt` - Trait extension for aggregating streamed SSE events into complete response payloads

**Request Builders** (`requests/`):
- `ResponsesRequest` / `ResponsesRequestBuilder`

**Auth** (`auth.rs`): `AuthProvider` handles API key and OAuth authentication.

**Common Types** (`common.rs`):
- `Prompt` - Input message type
- `ResponseEvent` - Streamed response events
- `ResponseStream` - Async event stream

### Things to Know

- Re-exports key types from `codex-client`
- Supports both streaming and non-streaming requests
- Rate limit handling in `rate_limits.rs`
- SSE fixture loading for testing via `stream_from_fixture()`
- This is primarily used for the legacy (non-ACP) backend

Created and maintained by Nori.
