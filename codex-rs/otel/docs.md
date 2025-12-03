# Noridoc: otel

Path: @/codex-rs/otel

### Overview

The `codex-otel` crate provides OpenTelemetry integration for Codex, enabling distributed tracing and telemetry export. It configures the OTLP exporter for sending traces to observability backends.

### How it fits into the larger codebase

Otel is used by TUI, exec, and app-server for observability:

- **Core** `otel_init.rs` uses this for provider initialization
- **All entry points** initialize OTEL tracing
- **Exports** to configured OTLP endpoints

### Core Implementation

Provides utilities for:
- OTLP exporter configuration
- Trace propagation
- Log export filtering

### Things to Know

**Configuration:**

OTEL export is configured via environment variables and config.toml settings.

**Filtering:**

`codex_export_filter()` in core determines which traces to export, avoiding noise from unimportant spans.

Created and maintained by Nori.
