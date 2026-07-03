# Crate Layering & Workspace Cleanup

Status: **draft — target layout agreed, gated on the `nori-acp` → `codex-core` import audit (§5)**
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
Layer 0 — leaves (independently useful, publishable to crates.io)
├── nori-acp-host     ACP host-side library: agent registry, subprocess spawn,
│                     JSON-RPC/stdio connection, session lifecycle, permission
│                     plumbing. No TUI, no ~/.nori, no codex-* deps.
├── nori-protocol     THE protocol crate, built on agent-client-protocol-schema.
│                     Minimal deps, no business logic.
└── mock-acp-agent    Conformance-test agent for ACP hosts and agent authors.

Layer 1 — headless runtime (the harness product)
├── nori-harness      Session runtime over nori-acp-host: backend reducer,
│                     transcript, undo, auto-worktree, hooks, message history.
│                     Embeddable without a terminal.
└── nori-config       ~/.nori loading, agent registry config, approval policy.
                      Runtime composition — no cargo features that move config.

Layer 2 — frontends (thin)
├── nori-tui          Rendering and input only. Drives nori-harness through its
│                     event interface; never imports nori-acp-host directly.
└── nori-cli          The `nori` binary: dispatch, plus headless exec/RPC mode
                      over the same harness.
```

Support crates keep their jobs where they are genuinely separate concerns
(sandboxing, login, git utils, PTY), but each must justify its dependents; none
may depend upward.

### Dependency rules

1. Arrows point down only. Layer 2 → Layer 1 → Layer 0. No sibling
   cross-imports within a layer.
2. `nori-tui` never imports `nori-acp-host` directly — all agent interaction
   flows through `nori-harness` events (the way upstream codex-tui drives core
   via app-server-client).
3. Nothing in Layers 0–1 may know a terminal exists (no ratatui, no ANSI).
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

## 5. Open question gating the plan: the codex-core audit

`nori-acp` depends on `codex-core`, `codex-protocol`, and `codex-rmcp-client`;
`translator.rs` bridges the Codex event model into ACP. Before slicing PRs we
need to know, import by import, what is actually used:

- [ ] What does `nori-acp` import from `codex-core`? (config? auth? exec/
      sandbox? MCP client? rollout?)
- [ ] What does `nori-tui` import from `codex-core` / `codex-protocol` that
      isn't already mediated by `nori-acp`?
- [ ] Which `codex-protocol` types leak into `nori-protocol` or the TUI via
      the translator?
- [ ] Is upstream Codex sync dead? (The squash-rename already made merges
      coarse. If dead, `codex-*` crates are owned code, eligible for deletion
      and renaming. If alive, surviving crates stay byte-stable under a
      clearly-marked vendored subtree.)

Audit findings land in §7 of this doc and convert §6's phases 3–5 into
concrete PR plans.

## 6. Sequencing

Each phase lands as one or more independent, net-negative PRs. Later phases
depend on earlier ones; the audit (§5) gates phases 3–5.

| Phase | Slice | Risk |
|-------|-------|------|
| 1 | Hygiene: delete repo-root debris and stale worktrees; rewrite `nori-rs/README.md` for Nori; fix stale naming claims in `docs.md`; remove crates/bins not on the `nori` binary path | none |
| 2 | Dissolve `codex-common` into its consumers; consolidate `utils/*` micro-crates into one or two | low |
| 3 | Sever `nori-acp` → `codex-core`: extract the genuinely-needed pieces (per audit), make the ACP schema the single protocol, retire `translator.rs` from the hot path | high |
| 4 | Split `nori-acp` into `nori-acp-host` (Layer 0) + `nori-harness` (Layer 1) + `nori-config`; delete the `nori-config` cargo feature | medium |
| 5 | Invert the TUI: move orchestration out of `tui/src/` into the harness; TUI consumes harness events only | high |
| 6 | Ecosystem: publish `nori-acp-host` + `mock-acp-agent`; document transcript format; add headless exec/RPC mode | low |

Ground rules for every slice: net-negative or neutral diff preferred; no
behavior change without a snapshot/e2e test proving it; `cargo build --bin
nori && cargo test -p tui-pty-e2e` green before merge; docs updated in the
same PR.

## 7. Audit findings

*(pending — populated by the §5 audit before phase 3 planning begins)*
