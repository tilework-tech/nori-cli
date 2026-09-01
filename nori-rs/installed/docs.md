# Noridoc: nori-installed

Path: @/nori-rs/installed

### Overview

- The installed crate persists local Nori CLI installation and launch state in
  `$NORI_HOME/.nori-install.json` and exposes the authenticated product-analytics
  reporter used by the CLI frontends.
- Local launch classification and network analytics are separate paths:
  [`track_launch`](src/lib.rs) updates installation state, while
  [`AnalyticsReporter`](src/analytics.rs) reports meaningful agent activity only
  after a prompt reaches the ACP transport.

### How it fits into the larger codebase

```text
nori-cli / nori-exec / nori-tui
              │ SessionMode + AnalyticsReporter
              v
         nori-installed
        /              \
install-state JSON      authenticated v1 envelope
                              │ Firebase bearer token
                              v
           login.norisessions.com/api/analytics/v1/events
                              │
                              v
                 Sessions OAuth proxy -> PostHog
```

- [`@/nori-rs/cli`](../cli/docs.md) assigns the public session mode:
  `interactive`, `cloud`, `exec`, or `acp`.
- [`@/nori-rs/exec`](../exec/docs.md) and [`@/nori-rs/tui`](../tui/docs.md)
  attach the reporter to each launched [`HarnessHandle`](../harness/docs.md).
  A logical session therefore owns its own one-shot activity boundary even when
  one TUI process launches more than one agent session.
- The shared login configuration at `~/.nori-config.json` supplies Firebase
  credentials. Current files use nested `auth.username`, `auth.idToken`,
  `auth.idTokenExpiresAt`, and `auth.refreshToken`. Legacy files with top-level
  `username` and `refreshToken` remain readable; when an `auth` object is
  present it is authoritative and the flat values are ignored. Identity and
  organization properties are not constructed in this crate; the authenticated
  ingress owns canonical email identity and organization expansion.
- The binding event contract and server-side identity rules live in the
  [monorepo analytics specification](https://github.com/tilework-tech/nori-monorepo/blob/main/docs/analytics.md).
- Existing Registrar support for older `noricli_*` payloads remains a server-side
  compatibility boundary. New releases do not emit that legacy format.

### Core Implementation

- [`state.rs`](src/state.rs) persists the anonymous client UUID, installed
  version, first-install time, last-launch time, install source, schema version,
  and durable analytics opt-out. [`track_launch_inner`](src/lib.rs) classifies
  first install, version change, ordinary session, and return after a long
  absence, then writes the updated state. Those classifications remain local
  lifecycle state and do not send network events.
- [`AnalyticsReporter::attach`](src/analytics.rs) decorates one harness handle
  with a callback. The harness invokes it once only after the first accepted user
  prompt has acquired its real ACP wire request ID. Merely launching, preparing,
  listing, or resuming a session does not report activity.
- The reporter sends `nori_agent_session_started` with `product = "nori"`,
  `surface = "cli"`, and the exact required `session_mode`. The v1 envelope also
  carries the released application version, a per-activity UUID, and a UTC
  timestamp. It does not carry prompts, paths, agent names, raw arguments,
  client-selected identity, or organization data.
- A currently valid nested `idToken` is used directly. When it is absent or
  expired and a nested or legacy flat refresh token exists,
  [`refresh_id_token`](src/analytics.rs) exchanges that token with Firebase
  before posting the event. Missing configuration, malformed human identity,
  and `nori-service:*` identities produce no request.
- Production release builds default to
  `https://login.norisessions.com/api/analytics/v1/events`. Debug builds send
  only when `NORI_ANALYTICS_URL` supplies an explicit endpoint, which is also the
  local test/development override.
- Capture asks [`std::thread::Builder`](src/analytics.rs) for a named background
  thread with its own current-thread Tokio runtime. Only a successfully spawned
  worker is registered for the final flush; thread-creation failure is discarded
  like every other analytics failure. The request and final
  [`AnalyticsReporter::flush`](src/analytics.rs) share a 250 ms bound, so read,
  refresh, serialization, transport, and response errors cannot change command
  output or exit status.

### Things to Know

- `NORI_NO_ANALYTICS=1` and the durable `opt_out` value in
  `$NORI_HOME/.nori-install.json` both disable authenticated capture. CI also
  skips capture unless an explicit `NORI_ANALYTICS_URL` is supplied.
- The event is once per logical agent session, not once per process and not once
  per prompt. A second prompt on the same attached handle is silent; a newly
  launched handle can report its own first transported prompt.
- Resume is not a session mode or a separate event. A resumed session contributes
  activity only when its first prompt in the current logical session reaches the
  ACP transport, using the frontend's `interactive` mode.
- [`analytics.rs`](src/analytics.rs) uses its explicitly named `reqwest` 0.13
  dependency for the JSON ingress and Firebase form exchange. The workspace's
  existing ACP/OAuth HTTP paths remain on `reqwest` 0.12.
- The persisted anonymous client UUID remains for installation-state and legacy
  compatibility purposes. It is not sent in the authenticated v1 envelope and
  is not used as the authenticated PostHog identity.

Created and maintained by Nori.
