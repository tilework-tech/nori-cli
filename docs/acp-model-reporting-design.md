# Design: ACP Model Reporting

## Problem

When nori-cli launches an ACP agent (e.g., `@anthropic-ai/claude-code`), the client has no
visibility into which underlying LLM model the agent is actually using. The TUI header and
`SessionConfiguredEvent.model` field both display the **agent name** (e.g.,
`@anthropic-ai/claude-code`), not the underlying model (e.g., `claude-sonnet-4-20250514`).

This matters because:
- Users want to know which model is processing their requests
- Model changes mid-session (via the model picker or agent-side switching) are not surfaced
- Transcript recordings and session metadata only capture the agent name, not the model

## Current State

### What the ACP spec provides

The `agent-client-protocol` crate (v0.9.4, schema v0.10.8) has two mechanisms for model
information:

**1. Unstable Session Model API** (`unstable_session_model` feature, already enabled):
- `NewSessionResponse.models: Option<SessionModelState>` — reports available models and current
  model on session creation
- `SessionModelState { current_model_id: ModelId, available_models: Vec<ModelInfo> }` — model
  state with ID, name, description
- `session/set_model` method — client-initiated model switching
- `LoadSessionResponse.models` — same on session load

**2. Stable Config Options with Model Category**:
- `NewSessionResponse.config_options: Option<Vec<SessionConfigOption>>` — can include a model
  selector with `category: SessionConfigOptionCategory::Model`
- `SessionUpdate::ConfigOptionUpdate` — agent-pushed notification when config (including model)
  changes
- Each `SessionConfigOption` contains a `SessionConfigKind::Select` with current value and
  available options

**3. `_meta` Extensibility** on every ACP message — arbitrary key-value metadata

### What nori-cli does today

- **Captures model state**: After `new_session()` / `load_session()`, `AcpModelState` is
  populated from `NewSessionResponse.models` (in `worker.rs:177-189`)
- **Stores model state**: `Arc<RwLock<AcpModelState>>` on `AcpConnection` (in
  `connection/mod.rs:172`)
- **Exposes model state**: `AcpConnection::model_state()` returns a clone, used by the TUI model
  picker (in `public_api.rs:207`)
- **Does NOT propagate to events**: `SessionConfiguredEvent.model` is set to `config.agent.clone()`
  — the agent name — in `spawn_and_relay.rs:198`
- **Does NOT handle `ConfigOptionUpdate`**: The `translate_session_update_to_events` function has
  no arm for `ConfigOptionUpdate`; it falls through to the catch-all `other =>` arm and is silently
  dropped (in `event_translation.rs:257-264`)

### The gaps

| Gap | Impact |
|-----|--------|
| `SessionConfiguredEvent.model` = agent name, not model ID | TUI header shows wrong info |
| No `ConfigOptionUpdate` handling | Agent-pushed model changes are dropped |
| No event for model changes mid-session | TUI cannot react to model switches |
| Transcript/session metadata records agent name | Rollout files lack model info |

## Design

### Approach: Use `SessionModelState` (unstable) as primary, `ConfigOptionUpdate` as fallback

The unstable model API is already integrated and provides structured, typed model information.
The stable `ConfigOptionUpdate` with `category: Model` serves as a complementary channel for
agents that don't implement the unstable API but do report model via config options.

### Changes

#### 1. Propagate model ID into `SessionConfiguredEvent`

**File**: `codex-rs/acp/src/backend/spawn_and_relay.rs`

After session creation, read `AcpModelState` and use `current_model_id` for the event's `model`
field instead of the agent name:

```rust
// After session creation and model state capture
let model_state = connection.model_state();
let model_display = model_state
    .current_model_id
    .as_ref()
    .map(|id| id.to_string())
    .unwrap_or_else(|| config.agent.clone());

let session_configured = SessionConfiguredEvent {
    model: model_display,
    // ... rest unchanged
};
```

This is backwards-compatible: if the agent doesn't report models (i.e., `current_model_id` is
`None`), we fall back to the agent name.

#### 2. Add `ModelChanged` event to `EventMsg`

**File**: `codex-rs/protocol/src/protocol/mod.rs`

Add a new event variant for mid-session model changes:

```rust
/// The underlying model used by the agent has changed.
ModelChanged(ModelChangedEvent),
```

```rust
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, TS)]
pub struct ModelChangedEvent {
    /// The new model identifier (e.g., "claude-sonnet-4-20250514").
    pub model: String,
    /// Human-readable display name for the model, if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}
```

#### 3. Handle `ConfigOptionUpdate` in event translation

**File**: `codex-rs/acp/src/backend/event_translation.rs`

Add an arm in `translate_session_update_to_events` for `ConfigOptionUpdate`:

```rust
acp::SessionUpdate::ConfigOptionUpdate(update) => {
    // Look for a config option with category "model"
    for option in &update.config_options {
        if option.category == Some(acp::SessionConfigOptionCategory::Model) {
            if let acp::SessionConfigKind::Select(select) = &option.kind {
                let model_id = select.current_value.to_string();
                let display_name = select.options.iter()
                    .find(|o| o.id.to_string() == model_id)
                    .map(|o| o.name.clone());
                return vec![EventMsg::ModelChanged(ModelChangedEvent {
                    model: model_id,
                    display_name,
                })];
            }
        }
    }
    vec![]
}
```

#### 4. Emit `ModelChanged` after `set_model` succeeds

**File**: `codex-rs/acp/src/backend/submit_and_ops.rs` (or wherever `set_model` result is
handled)

After a successful `set_model` call, emit a `ModelChanged` event so the TUI updates its header:

```rust
// After successful set_model
event_tx.send(Event {
    id: String::new(),
    msg: EventMsg::ModelChanged(ModelChangedEvent {
        model: model_id.to_string(),
        display_name: None, // Could look up from available_models
    }),
}).await.ok();
```

#### 5. Handle `ModelChanged` in the TUI

**File**: `codex-rs/tui/src/chatwidget/event_handlers.rs`

When receiving `ModelChanged`, update the session header to reflect the new model:

```rust
EventMsg::ModelChanged(event) => {
    // Update the session header with the new model name
    self.session_header.update_model(
        event.display_name.as_deref().unwrap_or(&event.model)
    );
}
```

#### 6. Record model in transcript metadata

**File**: `codex-rs/acp/src/backend/spawn_and_relay.rs`

When initializing the `TranscriptRecorder`, pass the resolved model name (from
`AcpModelState.current_model_id`) rather than the agent name. This ensures rollout/transcript
files contain the actual model used.

### Data Flow

```
ACP Agent Process
  │
  ├─ NewSessionResponse { models: SessionModelState { current_model_id: "claude-sonnet-4-..." } }
  │     │
  │     ▼
  │   AcpConnection::model_state (Arc<RwLock<AcpModelState>>)
  │     │
  │     ▼
  │   spawn_and_relay.rs: SessionConfiguredEvent { model: "claude-sonnet-4-..." }
  │     │
  │     ▼
  │   TUI session header: "claude-sonnet-4-..."
  │
  ├─ SessionUpdate::ConfigOptionUpdate { category: Model, current_value: "new-model" }
  │     │
  │     ▼
  │   event_translation.rs → EventMsg::ModelChanged { model: "new-model" }
  │     │
  │     ▼
  │   TUI session header updates
  │
  └─ Client calls session/set_model → success
        │
        ▼
      EventMsg::ModelChanged { model: "new-model-id" }
        │
        ▼
      TUI session header updates
```

### What agents need to do

For this to work, ACP agents must report their model. The level of support depends on the agent:

| Agent | Expected behavior |
|-------|-------------------|
| Agents implementing `unstable_session_model` | Report `SessionModelState` in `NewSessionResponse`. nori-cli picks up `current_model_id` automatically. |
| Agents using stable config options | Report a `SessionConfigOption` with `category: Model` in `NewSessionResponse.config_options`, and send `ConfigOptionUpdate` on changes. |
| Agents that do neither | nori-cli falls back to displaying the agent name (current behavior). No regression. |

### Edge Cases

- **Agent reports neither model API nor config options**: Fallback to agent name. No change from
  current behavior.
- **Agent reports both `SessionModelState` and `ConfigOptionUpdate`**: Prefer
  `SessionModelState.current_model_id` at session start. Honor `ConfigOptionUpdate` for mid-session
  changes. If both arrive simultaneously, last-write-wins.
- **Model name is opaque/internal**: Some agents may report internal model IDs (e.g.,
  `gpt-4o-2024-08-06`). Display as-is — the `display_name` field in `ModelInfo` or the config
  option `name` provides a human-friendly alternative.
- **Model changes during an active turn**: Buffer the `ModelChanged` event and apply it when the
  turn completes, to avoid confusing mid-stream model label changes.

### Non-goals

- **Forcing agents to report models**: This is opt-in. Agents that don't implement model reporting
  continue to work with the agent name displayed.
- **Proposing ACP spec changes**: This design works entirely within the existing ACP schema
  (v0.10.8). The `unstable_session_model` feature may stabilize independently.
- **Model-specific behavior in nori-cli**: We only report/display the model. We don't change
  behavior based on which model is reported.

### Migration / Rollout

1. All changes are additive — no breaking changes to existing events or APIs
2. The `ModelChanged` event is a new `EventMsg` variant; existing TUI code ignores unknown variants
3. Feature-gated behind `unstable` where it touches `SessionModelState` types
4. Agents that don't report models see zero behavioral change
