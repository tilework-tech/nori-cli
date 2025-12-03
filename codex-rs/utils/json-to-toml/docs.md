# Noridoc: utils/json-to-toml

Path: @/codex-rs/utils/json-to-toml

### Overview

The `codex-utils-json-to-toml` crate provides conversion from `serde_json::Value` to `toml::Value` for configuration handling.

### How it fits into the larger codebase

JSON-to-TOML is used for config processing:

- **Config system** may convert JSON config fragments to TOML
- **Enables** interoperability between formats

### Core Implementation

**Main Function:**

```rust
pub fn json_to_toml(v: JsonValue) -> TomlValue
```

**Mappings:**

| JSON Type | TOML Type |
|-----------|-----------|
| null | String ("") |
| bool | Boolean |
| number (int) | Integer |
| number (float) | Float |
| string | String |
| array | Array |
| object | Table |

### Things to Know

**Null Handling:**

JSON `null` becomes empty string since TOML has no null type.

**Number Handling:**

Integers preserved as integers, floats as floats. Falls back to string for exotic numbers.

Created and maintained by Nori.
