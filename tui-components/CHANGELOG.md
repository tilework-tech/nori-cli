# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - Unreleased

### Added
- Initial release of tui-components
- Shimmer component for animated text effects with configurable color palettes
- KeyHint component for keyboard shortcut display with platform-aware formatting
- Renderable trait with composable layout primitives:
  - ColumnRenderable for vertical stacking
  - RowRenderable for horizontal placement
  - InsetRenderable for padding
  - RenderableExt trait with helper methods
- ScrollState for managing scroll position and selection in lists
- PasteBurst for timing-based paste detection
- Line utilities for text manipulation
- Optional syntax-highlighting feature for bash code rendering (tree-sitter-based)
- Comprehensive test suite with 37 tests including snapshot tests
- Full rustdoc documentation with examples for all public APIs
