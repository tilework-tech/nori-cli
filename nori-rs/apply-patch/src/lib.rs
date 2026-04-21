mod application;
mod heredoc;
mod parser;
mod seek_sequence;
mod shell_parsing;
mod standalone_executable;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;
pub use application::AffectedPaths;
pub use application::ApplyPatchFileUpdate;
pub use application::apply_hunks;
pub use application::apply_patch;
pub use application::print_summary;
pub use application::unified_diff_from_chunks;
pub use application::unified_diff_from_chunks_with_context;
pub use heredoc::ExtractHeredocError;
pub use parser::Hunk;
pub use parser::ParseError;
use parser::UpdateFileChunk;
pub use parser::parse_patch;
pub use shell_parsing::maybe_parse_apply_patch;
pub use standalone_executable::main;
use thiserror::Error;

use shell_parsing::parse_shell_script;

/// Detailed instructions for gpt-4.1 on how to use the `apply_patch` tool.
pub const APPLY_PATCH_TOOL_INSTRUCTIONS: &str = include_str!("../apply_patch_tool_instructions.md");

const APPLY_PATCH_COMMANDS: [&str; 2] = ["apply_patch", "applypatch"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApplyPatchShell {
    Unix,
    PowerShell,
    Cmd,
}

#[derive(Debug, Error, PartialEq)]
pub enum ApplyPatchError {
    #[error(transparent)]
    ParseError(#[from] ParseError),
    #[error(transparent)]
    IoError(#[from] IoError),
    /// Error that occurs while computing replacements when applying patch chunks
    #[error("{0}")]
    ComputeReplacements(String),
    /// A raw patch body was provided without an explicit `apply_patch` invocation.
    #[error(
        "patch detected without explicit call to apply_patch. Rerun as [\"apply_patch\", \"<patch>\"]"
    )]
    ImplicitInvocation,
}

impl From<std::io::Error> for ApplyPatchError {
    fn from(err: std::io::Error) -> Self {
        ApplyPatchError::IoError(IoError {
            context: "I/O error".to_string(),
            source: err,
        })
    }
}

impl From<&std::io::Error> for ApplyPatchError {
    fn from(err: &std::io::Error) -> Self {
        ApplyPatchError::IoError(IoError {
            context: "I/O error".to_string(),
            source: std::io::Error::new(err.kind(), err.to_string()),
        })
    }
}

#[derive(Debug, Error)]
#[error("{context}: {source}")]
pub struct IoError {
    context: String,
    #[source]
    source: std::io::Error,
}

impl PartialEq for IoError {
    fn eq(&self, other: &Self) -> bool {
        self.context == other.context && self.source.to_string() == other.source.to_string()
    }
}

#[derive(Debug, PartialEq)]
pub enum MaybeApplyPatch {
    Body(ApplyPatchArgs),
    ShellParseError(ExtractHeredocError),
    PatchParseError(ParseError),
    NotApplyPatch,
}

/// Both the raw PATCH argument to `apply_patch` as well as the PATCH argument
/// parsed into hunks.
#[derive(Debug, PartialEq)]
pub struct ApplyPatchArgs {
    pub patch: String,
    pub hunks: Vec<Hunk>,
    pub workdir: Option<String>,
}

#[derive(Debug, PartialEq)]
pub enum ApplyPatchFileChange {
    Add {
        content: String,
    },
    Delete {
        content: String,
    },
    Update {
        unified_diff: String,
        move_path: Option<PathBuf>,
        /// new_content that will result after the unified_diff is applied.
        new_content: String,
    },
}

#[derive(Debug, PartialEq)]
pub enum MaybeApplyPatchVerified {
    /// `argv` corresponded to an `apply_patch` invocation, and these are the
    /// resulting proposed file changes.
    Body(ApplyPatchAction),
    /// `argv` could not be parsed to determine whether it corresponds to an
    /// `apply_patch` invocation.
    ShellParseError(ExtractHeredocError),
    /// `argv` corresponded to an `apply_patch` invocation, but it could not
    /// be fulfilled due to the specified error.
    CorrectnessError(ApplyPatchError),
    /// `argv` decidedly did not correspond to an `apply_patch` invocation.
    NotApplyPatch,
}

/// ApplyPatchAction is the result of parsing an `apply_patch` command. By
/// construction, all paths should be absolute paths.
#[derive(Debug, PartialEq)]
pub struct ApplyPatchAction {
    changes: HashMap<PathBuf, ApplyPatchFileChange>,

    /// The raw patch argument that can be used with `apply_patch` as an exec
    /// call. i.e., if the original arg was parsed in "lenient" mode with a
    /// heredoc, this should be the value without the heredoc wrapper.
    pub patch: String,

    /// The working directory that was used to resolve relative paths in the patch.
    pub cwd: PathBuf,
}

impl ApplyPatchAction {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Returns the changes that would be made by applying the patch.
    pub fn changes(&self) -> &HashMap<PathBuf, ApplyPatchFileChange> {
        &self.changes
    }

    /// Should be used exclusively for testing. (Not worth the overhead of
    /// creating a feature flag for this.)
    pub fn new_add_for_test(path: &Path, content: String) -> Self {
        if !path.is_absolute() {
            panic!("path must be absolute");
        }

        #[expect(clippy::expect_used)]
        let filename = path
            .file_name()
            .expect("path should not be empty")
            .to_string_lossy();
        let patch = format!(
            r#"*** Begin Patch
*** Update File: {filename}
@@
+ {content}
*** End Patch"#,
        );
        let changes = HashMap::from([(path.to_path_buf(), ApplyPatchFileChange::Add { content })]);
        #[expect(clippy::expect_used)]
        Self {
            changes,
            cwd: path
                .parent()
                .expect("path should have parent")
                .to_path_buf(),
            patch,
        }
    }
}

/// cwd must be an absolute path so that we can resolve relative paths in the
/// patch.
pub fn maybe_parse_apply_patch_verified(argv: &[String], cwd: &Path) -> MaybeApplyPatchVerified {
    // Detect a raw patch body passed directly as the command or as the body of a shell
    // script. In these cases, report an explicit error rather than applying the patch.
    if let [body] = argv
        && parse_patch(body).is_ok()
    {
        return MaybeApplyPatchVerified::CorrectnessError(ApplyPatchError::ImplicitInvocation);
    }
    if let Some((_, script)) = parse_shell_script(argv)
        && parse_patch(script).is_ok()
    {
        return MaybeApplyPatchVerified::CorrectnessError(ApplyPatchError::ImplicitInvocation);
    }

    match maybe_parse_apply_patch(argv) {
        MaybeApplyPatch::Body(ApplyPatchArgs {
            patch,
            hunks,
            workdir,
        }) => {
            let effective_cwd = workdir
                .as_ref()
                .map(|dir| {
                    let path = Path::new(dir);
                    if path.is_absolute() {
                        path.to_path_buf()
                    } else {
                        cwd.join(path)
                    }
                })
                .unwrap_or_else(|| cwd.to_path_buf());
            let mut changes = HashMap::new();
            for hunk in hunks {
                let path = hunk.resolve_path(&effective_cwd);
                match hunk {
                    Hunk::AddFile { contents, .. } => {
                        changes.insert(path, ApplyPatchFileChange::Add { content: contents });
                    }
                    Hunk::DeleteFile { .. } => {
                        let content = match std::fs::read_to_string(&path) {
                            Ok(content) => content,
                            Err(e) => {
                                return MaybeApplyPatchVerified::CorrectnessError(
                                    ApplyPatchError::IoError(IoError {
                                        context: format!("Failed to read {}", path.display()),
                                        source: e,
                                    }),
                                );
                            }
                        };
                        changes.insert(path, ApplyPatchFileChange::Delete { content });
                    }
                    Hunk::UpdateFile {
                        move_path, chunks, ..
                    } => {
                        let ApplyPatchFileUpdate {
                            unified_diff,
                            content: contents,
                        } = match unified_diff_from_chunks(&path, &chunks) {
                            Ok(diff) => diff,
                            Err(e) => {
                                return MaybeApplyPatchVerified::CorrectnessError(e);
                            }
                        };
                        changes.insert(
                            path,
                            ApplyPatchFileChange::Update {
                                unified_diff,
                                move_path: move_path.map(|p| effective_cwd.join(p)),
                                new_content: contents,
                            },
                        );
                    }
                }
            }
            MaybeApplyPatchVerified::Body(ApplyPatchAction {
                changes,
                patch,
                cwd: effective_cwd,
            })
        }
        MaybeApplyPatch::ShellParseError(e) => MaybeApplyPatchVerified::ShellParseError(e),
        MaybeApplyPatch::PatchParseError(e) => MaybeApplyPatchVerified::CorrectnessError(e.into()),
        MaybeApplyPatch::NotApplyPatch => MaybeApplyPatchVerified::NotApplyPatch,
    }
}
