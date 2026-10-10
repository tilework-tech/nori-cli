/context latency investigation — PR 640

Root cause: every instruction file constructed and dropped a full o200k_base tokenizer on the TUI event thread. Recognized Claude/Codex paths use BPE; the earlier mock provider/instruction inventory bypassed that path.

Fix: reuse a fallible process-wide tokenizer initializer; run each complete instruction-file inventory batch in spawn_blocking; preserve fresh reads and generation/conversation guards. No timeline algorithm change. Middle UI now uses one Current arithmetic row, checkpoints and one carry note.

Measurements: debug builds, same session, no concurrent heavy builds during measurements. Real CLI in isolated tmux, 110x70, eight inherited/local instruction files, local mock ACP with three distinct usage readings. CLI recording and target-specific debug tracing enabled before and after. Enter-to-visible timings include subprocess polling overhead. No real provider API latency included.

Live median: 6588.566ms before → 299.942ms after (21.97x).
Three-file instruction inventory warm median: 2306.582ms before → 21289us after (108.35x). First call after fix: 786.652ms in a fresh process; no OS cache flush. File edit verified fresh count.
10,000-turn live timeline query median 76.864ms; JSONL load 3091.596ms; reduction184.939ms. Load/reduction occur on resume, not each /context. Query clones checkpoint state under RwLock. Increasing messages from1KiB to16KiB at1000turns changes load time but not query time (~7ms).

Artifacts:
 before/ and after/: e2e.jsonl, instruction-inventory.jsonl, stages.jsonl, raw rolling CLI logs, recorded session events, driver transcript and untouched terminal captures.
 integration/harness.jsonl: 7 samples for 5 fixture sizes, 3 separate stages, correctness checked outside timers.
 logs/: RED/GREEN, builds and full final checks. Profiling tests are ignored in normal suites but were run manually here.
 drive.py, before/run.sh, after/run.sh: exact local live-driver commands, paths specific to this session.

Trace interpretation: inventory includes tokenization. tokenizer_us before fix includes constructor; encoding_us before fix also includes owned tokenizer destruction. history queue insertion includes unbounded-width render work; recording causes an additional unbounded-width render. context_render rows overlap insertion. Do not add nested timings. Visible-width rendering ~2ms; unbounded rendering ~50ms per render remains unchanged.

Reproduce from nori-rs with the CLI and mock agent built:
  NORI_CONTEXT_PROFILE_DIR=/path/to/artifacts cargo test -p nori-tui profile_context_instruction_inventory -- --ignored --nocapture --test-threads=1
  NORI_CONTEXT_PROFILE_DIR=/path/to/artifacts cargo test -p nori-harness --test context_timeline profile_load_reduce_and_live_snapshot -- --ignored --nocapture --test-threads=1
  cargo test -p tui-pty-e2e --test context_instructions -- --nocapture
  RUST_LOG=nori_context_profile=debug nori

NORI_CONTEXT_ENFORCE_BUDGET=1 opts into a250ms repeated-inventory budget. The normal real-file PTY regression enforces a1second warm command budget and fresh counts after two edits. Async widget regression enforces prompt dispatch under100ms and eventual inventory output.
