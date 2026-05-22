use std::path::Path;

pub(crate) fn is_git_marker(path: &Path) -> bool {
    path.is_file() || path.join("HEAD").is_file()
}
