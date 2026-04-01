# Noridoc: codex-api

Path: @/codex-rs/codex-api

### Overview

The codex-api crate provides high-level API clients for the OpenAI Responses API. It is a self-contained crate that includes its own HTTP transport layer (in `src/client/`) and typed request builders with SSE response handling. This crate is part of the legacy HTTP backend and is only compiled when `codex-core` enables the `legacy-http-backend` feature.

### How it fits into the larger codebase

- Used by `@/codex-rs/core/` (specifically `client.rs` and `api_bridge.rs`) for the legacy HTTP backend path
- `codex-core`'s `Cargo.toml` declares `codex-api` as an optional dependency, pulled in only by the `legacy-http-backend` feature
- No production downstream crates (`nori-tui`, `nori-cli`, `codex-acp`) depend on this crate -- only `codex-core`'s test suite enables it via `dev-dependencies`
- The HTTP transport layer (`src/client/`) was previously a separate `codex-client` crate; it is now inlined as a `pub(crate)` submodule with public re-exports from `lib.rs`

### Core Implementation

**HTTP Transport** (`src/client/`): Self-contained HTTP transport module providing `HttpTransport` trait, `ReqwestTransport` implementation, `RetryPolicy` with exponential backoff via `run_with_retry`, `Request`/`Response` types, and `RequestTelemetry` trait. These types are re-exported from `lib.rs` so downstream code sees them at crate root level.

**Provider Abstraction** (`provider.rs`): `Provider` configures API endpoint, auth, and retry behavior. All providers use the OpenAI Responses API wire protocol exclusively -- there is no wire format selector.

**Responses Client** (`endpoint/responses.rs`): `ResponsesClient` and `ResponsesOptions` for making Responses API calls.

**Stream Aggregation** (`endpoint/aggregate.rs`): `AggregateStreamExt` trait extension for aggregating streamed SSE events into complete response payloads.

**Request Builders** (`requests/`): `ResponsesRequest` / `ResponsesRequestBuilder` for constructing API requests.

**Auth** (`auth.rs`): `AuthProvider` handles API key and OAuth authentication.

**Common Types** (`common.rs`): `Prompt`, `ResponseEvent`, `ResponseStream`, `ResponsesApiRequest`.

### Things to Know

- The `src/client/` module is `pub(crate)` -- external consumers access its types through the re-exports in `lib.rs` (e.g., `codex_api::HttpTransport`, `codex_api::ReqwestTransport`)
- Rate limit handling lives in `rate_limits.rs`
- SSE fixture loading for testing via `stream_from_fixture()`
- The Chat Completions endpoint (`endpoint/chat.rs`) and Compact endpoint (`endpoint/compact.rs`) have been removed; the crate now only supports the Responses API

Created and maintained by Nori.
