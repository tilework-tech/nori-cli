# Motion component for Handroll

This branch applies only the additive `tui-components::motion` module from
[nori-cli PR #629](https://github.com/tilework-tech/nori-cli/pull/629) at
`2892d40bee59ae698cf6c9124f66409892e26f00` to Handroll’s existing dependency
revision `1acf0f2544eb8c1916304643ff957ca69ad5cce2`. It intentionally retains
the existing menu, picker, and detail rendering contracts.

The original module and tests are preserved. An additive `MotionBackground::sandbox_zoom`
constructor supplies the continuous sandbox-to-braille transition required by
Handroll, with endpoint and nonblank-intermediate-frame coverage. The interactive startup storybook lives in Handroll.
Consolidate back onto the main shared-component revision when Handroll adopts
the intervening menu and picker changes.
