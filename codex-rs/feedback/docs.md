# Noridoc: feedback

Path: @/codex-rs/feedback

### Overview

The `codex-feedback` crate provides a tracing writer for collecting feedback and diagnostic information during Codex sessions. It captures log output that can be displayed to users or included in bug reports.

### How it fits into the larger codebase

Feedback is used by TUI and app-server for diagnostics:

- **TUI** creates `CodexFeedback` and registers with tracing subscriber
- **App-server** similarly uses for feedback collection
- **Makes** diagnostic info available for UI display

### Core Implementation

`CodexFeedback` provides:
- `make_writer()` for tracing subscriber integration
- Collection of formatted log messages
- Access to collected feedback for display

### Things to Know

**Tracing Integration:**

Used as a tracing layer alongside file and OTEL layers:
```rust
let feedback = CodexFeedback::new();
let layer = tracing_subscriber::fmt::layer()
    .with_writer(feedback.make_writer());
```

**Usage:**

Collected feedback can be shown in status displays or included in error reports.

Created and maintained by Nori.
