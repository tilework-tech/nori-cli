# Noridoc: codex-backend-openapi-models

Path: @/codex-rs/codex-backend-openapi-models

### Overview

The `codex-backend-openapi-models` crate contains auto-generated OpenAPI model types for the OpenAI backend API. These types are generated from the OpenAPI specification and provide strongly-typed request/response structures.

### How it fits into the larger codebase

This crate provides API types used by backend communication:

- **Core** uses for API request/response serialization
- **Backend client** uses for type-safe API calls
- **Generated code** - not hand-written

### Core Implementation

The crate re-exports generated models from `src/models/`. These are populated by a regeneration script from OpenAPI specs.

### Things to Know

**Generated Code:**

Contains no hand-written types. Models are regenerated when API spec changes.

**Lint Exceptions:**

Allows `clippy::unwrap_used` and `clippy::expect_used` since generated code often violates workspace lints.

**Dependencies:**

Uses serde with `derive` feature and `serde_with` for serialization customization.

Created and maintained by Nori.
