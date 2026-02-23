# Noridoc: codex-client

Path: @/codex-rs/codex-client

### Overview

The codex-client crate provides low-level HTTP transport and SSE streaming utilities for API communication. It handles retries, error handling, and telemetry for HTTP requests.

### How it fits into the larger codebase

Used by `@/codex-rs/codex-api/` as the transport layer for API requests. Provides the foundation for both the legacy HTTP backend and any direct API calls.

### Core Implementation

**HttpTransport Trait**: Abstraction over HTTP clients:
- `ReqwestTransport` - Default implementation using `reqwest`
- Supports custom transports for testing

**SSE Streaming** (`sse.rs`): `sse_stream()` parses Server-Sent Events from HTTP responses into typed event streams.

**Retry Logic** (`retry.rs`):
- `RetryPolicy` - Configures retry behavior
- `run_with_retry()` - Executes with automatic retries
- `backoff()` - Exponential backoff implementation

**Telemetry** (`telemetry.rs`): `RequestTelemetry` tracks timing and success metrics.

**Error Types** (`error.rs`):
- `TransportError` - HTTP/network errors
- `StreamError` - SSE parsing errors

### Things to Know

- The `ReqwestTransport` is the default production transport
- SSE streams handle reconnection and event parsing
- Retry policies are configurable per-request
- Request/response abstractions allow for easy mocking

Created and maintained by Nori.
