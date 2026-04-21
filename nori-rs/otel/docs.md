# Noridoc: codex-otel

Path: @/nori-rs/otel

### Overview

The otel crate provides OpenTelemetry integration for distributed tracing. When enabled, it propagates trace context through HTTP headers and manages telemetry providers.

### How it fits into the larger codebase

Used by `@/nori-rs/core/` (`otel_init.rs`) to initialize telemetry. Provides trace context propagation for API requests.

### Core Implementation

**OtelProvider** (`otel_provider.rs`, feature-gated): When the `otel` feature is enabled:
- `from(settings)` - Creates provider from configuration
- `headers(span)` - Extracts trace context headers for propagation

**OtelSettings** (`config.rs`): Configuration for OpenTelemetry including endpoint, service name, etc.

**Event Manager** (`otel_event_manager.rs`): Manages telemetry event lifecycle.

**Stub Implementation** (when `otel` feature disabled): Returns empty headers and `None` provider.

### Things to Know

- The `otel` feature must be enabled for actual telemetry
- Without the feature, all operations are no-ops
- Headers are extracted from tracing spans for context propagation
- Configuration is loaded from environment or config file

Created and maintained by Nori.
