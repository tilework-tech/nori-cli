# Motion measurement status

The initial investigation was interrupted. Its broad headless/live runs reported
frame-budget overruns at larger terminal sizes, but their raw traces were stored
in temporary files and are no longer available. The planned CPU sampling for
this checkout did not produce a retained, verified profile. The methodology is
a record of that protocol, not a claim that every listed artifact is present.

A subsequent small comparison against `codex/animated-onboarding` is retained in
[the comparison directory](../motion-comparison/README.md), including raw frame
CSVs, exact commands, binary hashes, and interpretation limits. That comparison
uses existing release binaries with different visual/sampling policies and no
terminal I/O; it is not a measurement of displayed FPS.

The comparison describes the implementations before the API cleanup. Its data
files are a historical baseline; rerunning its script replaces those files with
measurements of the binaries currently at the recorded paths. Preserve that
baseline and use a separate output directory for future experiments.

The cleanup separates the stateless rendering API, optional controller, palette,
and geometry modules. It retains the existing sampling schedule and scene
formulas. No post-cleanup performance improvement is claimed. See the
[integration guide](../../reference/motion-background.md) for the current API.
