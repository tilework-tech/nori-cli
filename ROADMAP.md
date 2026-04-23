# Nori CLI ACP Roadmap

This roadmap is about Nori's ACP client work, not ACP in the abstract. The goal is to keep the client simple, fast, and predictable while we fill in the highest-value protocol features.

## Current Focus

### Core Session Lifecycle

- DONE: `load`, `list`, `resume`, and `undo`
- TODO: `fork` and tree-oriented session flows
- TODO: hook resume up to a top-level subcommand
- TODO: improve resume performance

This is the main workstream right now. Session lifecycle is the foundation for everything else, so we want it to feel complete before we spread effort across more protocol surface.

## Next Features

These are the next highest-leverage additions after the core lifecycle work. They make Nori more useful without adding much conceptual weight.

### Session Configuration

- custom config options such as thinking or effort level
- mode-style options such as plan versus build

### Agent Discovery

- support the official ACP Registry for discovering custom agents

## Future Features

### Agent-Driven Auth

- auth methods
- logout

We want these, but they are behind lifecycle, configuration, and registry support.

## Already Working Well

- session loading
- local session continuity and resume flows
- undo support
- custom agent registration through local config

These areas are good enough to build on, even where we may still want cleaner or more native ACP implementations underneath.

## ACP Feature Reference

This section is the dependency-facing catalog. It exists to keep the roadmap grounded in the current ACP spec and draft surface, but it is not the main structure of the project roadmap.

| Feature | ACP status | Nori status |
| --- | --- | --- |
| `session/load` | stable baseline | done |
| `session/list` | stable | done |
| Session Config Options | stable | near term |
| `session_info_update` | stable | later |
| ACP Registry | stable | near term |
| `session/resume` | unstable landed | done |
| `session/fork` | unstable landed | todo |
| `session/close` | unstable landed | handled |
| `additionalDirectories` | unstable landed | partial |
| auth methods | unstable landed | planned |
| `logout` | unstable landed | planned |
| `messageId` and `userMessageId` | unstable landed | partial |
| usage updates | unstable landed | partial |
| boolean config options | unstable landed | near term |
| elicitation | unstable landed | later |
| NES | unstable landed | later |
| `session/delete` | draft only | not planned near term |
| custom provider endpoints | draft only | not planned near term |
| diff-delete metadata | draft only | not planned near term |
| proxy-chains | draft only | not planned near term |
| subagents | not first-class ACP today | not planned near term |

## Notes

- This reflects our current understanding as of April 2026.
- "Done" in this roadmap means done enough for Nori's current product goals, not necessarily that every ACP-native implementation detail is perfect.
- We should keep preferring fewer special cases and more native ACP behavior as we iterate.
