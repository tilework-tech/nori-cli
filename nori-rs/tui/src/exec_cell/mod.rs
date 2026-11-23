//! Rendering helpers for shell commands in tool cells.

mod output_cap;
#[cfg(test)]
mod output_cap_tests;
mod render;
#[cfg(test)]
mod tests;

pub(crate) use output_cap::CappedLines;
pub(crate) use output_cap::cap_lines;
pub(crate) use output_cap::hidden_lines_marker;
pub(crate) use render::compact_command;
pub(crate) use render::full_command_lines;
pub(crate) use render::is_read_only;
pub(crate) use render::last_output_line;
pub(crate) use render::reads_files;
pub(crate) use render::spinner;
