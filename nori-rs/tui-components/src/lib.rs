//! Reusable, domain-free Ratatui components for Nori applications.
//!
//! This crate deliberately does not own terminal setup or an event loop.
//! Consumers translate their input events into component actions, update
//! caller-owned state, and render the resulting widgets.

pub mod detail;
pub mod markdown;
pub mod picker;
pub mod primitives;
pub mod theme;

pub use detail::DetailBackground;
pub use detail::DetailEntry;
pub use detail::DetailPane;
pub use detail::DetailTone;
pub use detail::LabelWidth;
pub use detail::ProviderKind;
pub use markdown::Markdown;
pub use markdown::StreamingMarkdown;
pub use picker::Picker;
pub use picker::PickerAction;
pub use picker::PickerColumn;
pub use picker::PickerColumnWidth;
pub use picker::PickerDensity;
pub use picker::PickerDetail;
pub use picker::PickerItem;
pub use picker::PickerLoadState;
pub use picker::PickerMode;
pub use picker::PickerOutcome;
pub use picker::PickerState;
pub use picker::SearchMode;
pub use primitives::EmptyState;
pub use primitives::KeyHint;
pub use primitives::KeyHints;
pub use primitives::MessageLevel;
pub use primitives::SemanticMessage;
pub use theme::Theme;
