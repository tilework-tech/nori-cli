# Noridoc: codex-api

Path: @/codex-rs/codex-api

### Overview

The codex-api crate provides high-level API clients for AI provider APIs. It wraps the low-level transport from `codex-client` with typed request builders and response handling for chat and responses endpoints.

### How it fits into the larger codebase

Used by `@/codex-rs/core/` for the legacy HTTP backend (non-ACP mode). Provides clients for OpenAI-compatible APIs.

### Core Implementation

**Provider Abstraction** (`provider.rs`):
- `Provider` - Configures API endpoint, auth, and wire format
- `WireApi` - Specifies protocol (Chat, Responses)

**Chat Client** (`endpoint/chat.rs`):
- `ChatClient` - Standard OpenAI chat completions API
- `AggregateStreamExt` - Aggregates streaming responses

**Responses Client** (`endpoint/responses.rs`):
- `ResponsesClient` - OpenAI Responses API
- `ResponsesOptions` - Configuration options

**Compact Client** (`endpoint/compact.rs`):
- `CompactClient` - For conversation compaction

**Request Builders** (`requests.rs`):
- `ChatRequest` / `ChatRequestBuilder`
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
