# Noridoc: responses-api-proxy

Path: @/codex-rs/responses-api-proxy

### Overview

The `codex-responses-api-proxy` crate provides a proxy server for the OpenAI Responses API. It's an internal tool used for development and testing purposes.

### How it fits into the larger codebase

Responses API proxy is invoked via hidden CLI command:

- **CLI** `codex responses-api-proxy` (hidden subcommand)
- **Used** for development testing
- **Not** intended for production use

### Core Implementation

Implements an HTTP proxy that:
- Forwards requests to OpenAI
- Allows inspection of request/response
- Supports testing scenarios

### Things to Know

This is an internal development tool not exposed in public CLI help.

Created and maintained by Nori.
