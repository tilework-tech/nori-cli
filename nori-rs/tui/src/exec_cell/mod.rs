mod render;

#[derive(Clone, Debug, Default)]
pub(crate) struct CommandOutput {
    pub(crate) exit_code: i32,
    pub(crate) aggregated_output: String,
}

pub(crate) use render::OutputLinesParams;
pub(crate) use render::TOOL_CALL_MAX_LINES;
pub(crate) use render::limit_lines_from_start;
pub(crate) use render::output_lines;
pub(crate) use render::spinner;
pub(crate) use render::truncate_lines_middle;
