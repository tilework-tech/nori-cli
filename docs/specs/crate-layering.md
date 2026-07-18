# Crate Layering & Workspace Cleanup

Status: **draft — target layout agreed; import audit complete (§7), slices ready for PR planning**
Created: 2026-07-03

This document tracks the target crate layout for `nori-rs` and the sequence of
slices that get us there. It is the durable home for the restructure; individual
PR plans go in `docs/plans/` and link back here.

## 1. Identity: Nori is a harness, not an agent

Pi and Codex are *agents* that grew frontends. Nori is the inverse: a
*host/harness* whose one job is being the universal terminal frontend for any
ACP agent. `AGENTS.md` already states the constraint bluntly: *"We only care
about the ACP backend and the code that compiles into the nori bin."*

The crate layout should make that identity legible: a small, publishable
ACP-host library at the bottom; a headless session harness in the middle; thin
frontends on top. Everything inherited from the Codex fork that serves the
*agent* identity (the engine that talks to model APIs directly) is a candidate
for removal, not maintenance.

Design principles borrowed deliberately:

- **From pi:** pure-leaf libraries with zero internal deps that are
  independently useful; a tiny headless runtime (pi's agent-core is ~8k LOC);
  exactly one product crate that composes the layers; extension/SDK surfaces
  treated as products, with an explicit list of things core does *not* do.
- **From upstream Codex:** a written "resist adding code to core" rule;
  protocol types isolated in a minimal-dependency crate; frontends drive the
  engine through a client interface rather than importing it; small crate API
  surfaces, private-by-default.
- **Unix:** each crate does one thing; composition happens at the binary, not
  by cross-imports between siblings.

## 2. Current state (2026-07-03)

32 workspace members, ~210k LOC. `nori-tui` (77k) + `nori-acp` (34k) +
`codex-core` (24k) are 64% of the code. One shipped artifact: the `nori` binary
from crate `nori-cli`, wrapped by the `nori-ai-cli` npm package.

Observed problems, in rough order of cost:

1. **TUI/harness fusion.** 51 files in `tui/src/` reference `nori_acp`;
   `tui/src/nori/` and `tui/src/chatwidget/` mix rendering with session
   orchestration, agent picking, and config. The TUI is unshippable as a thin
   frontend and the harness is unusable without a terminal.
2. **`nori-acp` is three crates in a trench coat:** an ACP host-side protocol
   client (registry, connection, subprocess lifecycle), a session runtime
   (backend reducer, transcript, undo, auto-worktree, hooks, message history),
   and Nori config (`~/.nori` loading, approval policy).
3. **Protocol sprawl.** Four type crates coexist — `codex-protocol`,
   `codex-app-server-protocol`, `mcp-types`, `nori-protocol` — with
   `acp/src/translator.rs` hand-bridging the Codex model to the ACP schema.
4. **`codex-core` as a load-bearing god crate** (7 dependents, including
   low-level crates like `linux-sandbox` and `arg0`) despite the AGENTS.md
   position that the codex backend is out of scope.
5. **Compile-time boundary shifting.** The `nori-config` cargo feature on
   `nori-tui` switches whether config comes from `nori-acp` or `codex-core`.
   Boundaries should be fixed at design time, not per-build.
6. **Junk drawers and fragments.** `codex-common` (depends on core *and* is
   used by tui/cli); seven `utils/*` micro-crates, several under 100 LOC.
7. **Vestigial surface.** Crates not on the shipped binary's path
   (`stdio-to-uds`, standalone bins), a `nori-rs/README.md` that is still the
   verbatim upstream Codex README, and stale naming claims in `docs.md`.

## 3. Target layout

```
Layer 0 — protocol substrate (independently useful, publishable to crates.io)
└── nori-protocol     The public boundary types crate and sole ACP schema import
                      choke point. It re-exports the ACP schema, carries raw ACP
                      agent→client envelopes, and defines only the small set of
                      Nori-owned events ACP does not cover. Minimal deps, no
                      reducers, normalization, presentation, or other business
                      logic. See docs/specs/protocol-unification.md.

Layer 1 — configuration
└── nori-config       ~/.nori loading, agent registry config, approval policy.
                      Runtime composition — no cargo features that move config.

Layer 2 — ACP host
└── nori-acp-host     ACP host-side library over nori-protocol and nori-config:
                      agent registry, subprocess spawn, JSON-RPC/stdio
                      connection, session lifecycle, permission plumbing.

Layer 3 — headless runtime (the harness product)
└── nori-harness      Session runtime over nori-acp-host: backend reducer,
                      transcript, undo, auto-worktree, hooks, message history.
                      Embeddable without a terminal UI.

Layer 4 — frontends (thin)
├── nori-tui          Rendering and input only. Drives nori-harness through its
│                     event interface; never imports nori-acp-host directly.
└── nori-cli          The `nori` binary: dispatch, plus headless exec/RPC mode
                      over the same harness.

Test support (outside the product dependency chain)
└── mock-acp-agent    Conformance-test agent for ACP hosts and agent authors.
```

Support crates keep their jobs where they are genuinely separate concerns
(sandboxing, login, git utils, PTY), but each must justify its dependents; none
may depend upward.

### Dependency rules

1. Arrows point down only. Layer 4 → Layer 3 → Layer 2 → Layer 1 → Layer 0. No
   sibling cross-imports within a layer.
2. `nori-tui` never imports `nori-acp-host` directly — all agent interaction
   flows through `nori-harness` events (the way upstream codex-tui drives core
   via app-server-client).
3. Nothing in Layers 0–3 may depend on ratatui or render terminal presentation
   (including ANSI styling). ACP terminal operations remain a host concern.
4. New functionality lands in a new module or crate before it grows an
   existing one ("resist adding code to core", inherited and kept).
5. No cargo feature may change which crate owns a responsibility.

### Non-goals

- **A generic TUI widget library.** Retrofitting 77k LOC of ratatui code into a
  reusable pi-tui-style leaf is high cost, low demand. The valuable extraction
  is the harness, not the widgets.
- **Preserving the codex engine.** Per AGENTS.md, the codex backend and codex
  bin are out of scope. Inherited `codex-*` crates survive only if the shipped
  `nori` binary needs them.
- **An in-process plugin system.** Extensibility comes from the ACP boundary
  itself (bring-your-own agent) and from hooks; not from a Rust plugin API.

## 4. What this offers ecosystem builders

- **`nori-acp-host` on crates.io** — the ACP ecosystem has agent-side crates
  but no maintained host-side library. This is the missing piece for anyone
  building a terminal, IDE panel, bot, or CI harness that drives ACP agents.
- **`mock-acp-agent` on crates.io** — conformance testing for both sides of
  the protocol.
- **`nori-harness` as the embeddable session runtime** — sessions, transcripts,
  undo, worktrees, hooks over any ACP agent, no terminal required.
- **A headless `nori exec` / RPC mode** — scriptable from CI and other
  languages, mirroring pi's rpc-mode and codex's exec mode.
- **A documented transcript/session format** (like pi's `session-format.md`).

## 5. The codex-core question — answered

The import-level audit (findings in §7) settled the questions that gated
phases 3–5:

- **codex-core is not an engine anymore.** The agent loop, model client, and
  conversation manager were already stripped (#196, #230, #438). What remains
  is a 22.5k-LOC utility/config grab-bag; roughly a third to half of it is
  unreferenced by the `nori` binary.
- **The current codex-protocol path is load-bearing but not the target.** Its
  `Event`/`EventMsg`/`Op` vocabulary and ACP translator currently sit on the hot
  path, but they duplicate or distort the ACP boundary. The approved follow-up
  is a hard cut: expose raw ACP aggregates through `nori-protocol`, retain only
  Nori-owned concerns there, migrate query operations to typed harness methods,
  and delete `codex-protocol`. See `docs/specs/protocol-unification.md` for the
  normative ownership rule and deletion inventory.
- **Upstream sync is dead.** No `upstream` remote exists and there have been
  zero merges from openai/codex since the squash-rename (#443). Deleting and
  renaming inherited crates carries no merge cost. Convention going forward:
  rename `codex-*` crates as they are adopted/touched, not in churn-only PRs.

## 6. Sequencing — PR-sized slices

Each slice is an independent PR (or small PR train), ordered so every one
lands green and net-negative or neutral. Detailed per-slice implementation
plans go in `docs/plans/` as each is picked up.

| # | Slice | Contents | Risk | Est. LOC |
|---|-------|----------|------|---------:|
| A | Repo hygiene | Delete repo-root debris and stale `.worktrees` entries; rewrite `nori-rs/README.md` for Nori; fix stale naming claims in `docs.md` | none | −large |
| B | Dead-weight purge in codex-core | Delete `rollout` (2.1k — nori-acp has its own transcript recorder), `turn_diff_tracker`, `command_safety`, and other unreferenced modules; confirm transitive deps with a dead-code pass | low | −8–10k |
| C | Kill the `nori-config` feature | Delete the `not(nori-config)` cfg branches (~120 sites, 18 files — the dead legacy codex-config path), then remove the feature so nori config is the only path | low | −1–2k |
| D | Un-detour protocol imports | Rewire `codex_core::protocol::*` (178+ refs in tui, plus cli) to import `codex_protocol` directly; drop the re-exports from core's lib.rs | low | ~0 |
| E | Sever nori-acp → codex-core | Extract the six leaf helpers (`user_notification`, `custom_prompts::discover_prompts_in`, `parse_command`, `util::create_patch_with_context`, `compact` constants) plus `config::types::McpServerConfig` into their target crates; acp's only remaining codex deps are protocol + rmcp OAuth store | medium | ~0 |
| F | Extract config/auth | Pull codex-core's `config` subtree (6.2k, the biggest live consumer) and `auth` into `nori-config` / auth home; whatever codex-core still holds after B+E+F gets dissolved or renamed | medium | ~0 |
| G | Split nori-acp | `nori-acp-host` (registry, connection, subprocess, wire) + `nori-harness` (backend reducer, transcript, undo, worktrees, hooks) + config move; completed crate split, with protocol unification now specified separately | medium | ~0 |
| G2 | Unify protocol boundary | Re-export ACP schema from `nori-protocol`; emit raw ACP envelopes plus the small Nori event branch; replace the generic operation bus with typed harness methods; delete `codex-protocol` after the configuration rework and deletion review gate | high | −net |
| H | Invert the TUI | Move orchestration out of `tui/src/nori/` and `chatwidget/` into the harness; TUI consumes harness events only (dependency rule 2 becomes enforceable) | high | −net |
| I | Ecosystem surfaces | Publish `nori-acp-host` + `mock-acp-agent` to crates.io; document transcript/session format; add headless exec/RPC mode | low | +small |

Also folded in along the way: dissolve `codex-common` into its consumers and
consolidate the `utils/*` micro-crates (opportunistically, in whichever slice
touches them last).

Ground rules for every slice: net-negative or neutral diff preferred; no
behavior change without a snapshot/e2e test proving it; `cargo build --bin
nori && cargo test -p tui-pty-e2e` green before merge; docs updated in the
same PR.

## 7. Audit findings (2026-07-03)

Import-level audit of Nori-owned crates → inherited Codex crates. Verdict per
dependency edge:

| Edge | Verdict | Evidence |
|------|---------|----------|
| nori-acp → codex-core | **EXTRACT** | Six leaf helpers only: `user_notification` (`UserNotifier`, `AwaitingApproval`/`Idle`), `custom_prompts::discover_prompts_in`, `parse_command`, `util::create_patch_with_context`, two `compact` string constants, and `config::types::{McpServerConfig, McpServerTransportConfig}` (shared with tui — belongs in the config crate). No engine usage anywhere. |
| nori-acp → codex-protocol | **CURRENTLY LIVE → DELETE** | The audit correctly found `codex_protocol::{Event, EventMsg, Op}` and the ACP translator on every agent's hot path. Subsequent protocol design rejected that second vocabulary: ACP owns agent↔client semantics, `nori-protocol` re-exports the schema and adds only Nori concerns, and the Codex crate is deleted by a hard cut. Implementation waits for the configuration rework and refreshed deletion gate. |
| nori-acp → codex-rmcp-client | **KEEP** (or extract OAuth store) | Only the MCP OAuth token persistence (`load_oauth_tokens`/`save_oauth_tokens` etc.) in `connection/mcp.rs`; self-contained. |
| nori-acp → mcp-types | **DELETE** | Zero usage; not even in acp's Cargo.toml. |
| nori-tui → `codex_core::protocol::*` | **DELETE detour** | 178+ refs are re-exports of `codex_protocol`; rewire directly (slice D). |
| nori-tui `not(nori-config)` branches | **DELETE** | ~120 cfg sites across 18 files gate a legacy codex-core config path that is dead in the shipped bin (feature is default-on) (slice C). |
| nori-tui → codex-core config/auth/sandbox/git_info | **KEEP → extract later** | Real functionality with no ACP equivalent: `config` subtree (6.2k LOC), `AuthManager`/`CodexAuth`, `get_platform_sandbox`, `git_info`, `otel_init`, `project_doc`, `model_family` (slice F). |
| nori-tui → codex-common | **KEEP** | Small presentation helpers genuinely used (approval/model presets, fuzzy_match, elapsed). Dissolve opportunistically. |
| nori-cli → codex-core | **KEEP, isolated** | No legacy engine path exists in the bin — every subcommand ends in `nori_tui::run_main` driving the ACP backend. codex-core supplies config, auth (`login` feature), and the `nori sandbox` debug helpers (confined to `cli/src/debug_sandbox.rs`). `codex-arg0` stays as the multi-call entry shim. |
| codex-core dead weight | **DELETE** | `rollout` (2.1k — superseded by `acp/src/transcript/recorder.rs`), `turn_diff_tracker` (896), `command_safety` (839), `exec` (1.1k), `truncate`, `text_encoding`, `shell`, `bash`, `model_provider_info`, `openai_model_info`, more — ~8–10k LOC (~40%) unreferenced by acp/tui/cli, pending a transitive-dep check (slice B). |
| Upstream sync | **DEAD** | Only remote is `origin` (tilework-tech/nori-cli); no merges from openai/codex since squash-rename #443. Renames and deletions are free. |
