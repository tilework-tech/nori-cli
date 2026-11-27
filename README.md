# Host audit evidence (0.30.0-next.15 / main 214d79b0)

- `FINDINGS.md`: verdict, findings by impact, workarounds, what was and was not tested.
- `harness/audit/run.sh`: repeatable reproduction. `harness/audit/run.sh <nori> <out>`.
- `harness/audit/setup.py`, `harness/audit/tt.sh`: synthetic homes and tmux helpers.
- `harness/fixtures.py`, `harness/lantern_adapter.py`, `harness/term.py`: copied unchanged from `nori/host-tui-acceptance`.
- `results/harness-run/`: one full harness run (23/23 observed), with ledgers.
- `results/exploratory/`: manual captures cited in `FINDINGS.md` (`r1/…` = `r1-default-claude`, `r2/…` = `r2-default-gemini-missing`).

All data is synthetic. Runtime homes and host tokens are not committed.
