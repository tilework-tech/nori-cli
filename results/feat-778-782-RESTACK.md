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
