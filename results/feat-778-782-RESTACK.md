# #778–#782: tested heads vs current heads

These results were taken at the main-based heads below. During the run, the stack was rebased onto the launcher (#784 c8f3ee3b, on #777 5c72765d).

| PR | Tested | Current | Host-side patch (harness, host-runtime, protocol, acp-host) | TUI patch |
|---|---|---|---|---|
| #778 | a2e02a80 | 38f82990 | identical (`git patch-id`) | changed |
| #779 | a2b7848d | b25c4587 | identical | changed |
| #780 | 2f11ea33 | c54ce288 | identical | changed |
| #781 | 3b5f95a5 | db931955 | identical | changed |
| #782 | bd7d9f54 | 36a2eaa4 | identical | changed |

- **Still valid at the current heads:** the wire-probe results (`probes*.json`) and findings #778 F1 and #779 L1.
- **Re-run at the current heads:** the plain-`nori` K scenarios, S34m, the matrix, and every joined-terminal check.

## Update: restacked heads of 20:15 UTC

The heads above were replaced by #778 `e1a1e4b4` … #785 `ef209c3e`. For their verdict, and for the shell patch `da3f2a53`, see [`feat-778-785-VERDICT.md`](feat-778-785-VERDICT.md). `feat-785-a53c5fd7/` holds the intermediate run at the 19:00 heads (`/goal`, `/context` and `/compact` were refused in `nori attach` there).
