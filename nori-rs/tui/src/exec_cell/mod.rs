//! Rendering helpers for shell commands in tool cells.

mod render;
#[cfg(test)]
mod tests;

pub(crate) use render::compact_command;
pub(crate) use render::full_command_lines;
pub(crate) use render::is_read_only;
pub(crate) use render::last_output_line;
pub(crate) use render::reads_files;
pub(crate) use render::spinner;
