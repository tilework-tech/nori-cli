//! Display-oriented structure of a shell command line.
//!
//! `summarize` splits a script into top-level segments (joined by `&&`, `||`,
//! `;` or newlines), each made of pipeline stages. It is tolerant: parse
//! errors keep whatever commands tree-sitter recovered.

use std::ops::Range;

use tree_sitter::Node;
use tree_sitter::Parser;
use tree_sitter_bash::LANGUAGE as BASH;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellSegment {
    /// Operator joining this segment to the previous one (`&&`, `||`, `;`).
    pub separator: Option<&'static str>,
    pub stages: Vec<ShellStage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellStage {
    /// Byte range of the command name.
    pub name: Range<usize>,
    /// Byte range of the script an interpreter runs (`python3 tools/gen.py`).
    pub script: Option<Range<usize>>,
    /// Byte range of the whole stage, redirections included.
    pub text: Range<usize>,
    /// Arguments follow the name (beyond the script, if any).
    pub has_args: bool,
    /// Line count of a heredoc body fed to this stage.
    pub heredoc_lines: Option<usize>,
    /// Output is redirected to a file other than `/dev/null`.
    pub writes_file: bool,
    /// Parts of the stage cannot be read statically: a parse error, a
    /// command or process substitution, a compound statement, or a redirect
    /// with no command.
    pub opaque: bool,
}

pub fn summarize(src: &str) -> Option<Vec<ShellSegment>> {
    let mut parser = Parser::new();
    parser.set_language(&BASH.into()).ok()?;
    let tree = parser.parse(src, None)?;
    let mut segments = Vec::new();
    collect_segments(tree.root_node(), src, None, &mut segments);
    if tree.root_node().has_error() {
        for stage in segments.iter_mut().flat_map(|segment| &mut segment.stages) {
            stage.opaque = true;
        }
    }
    (!segments.is_empty()).then_some(segments)
}

const SUBSTITUTIONS: &[&str] = &["command_substitution", "process_substitution"];

const INTERPRETERS: &[&str] = &[
    "python", "python3", "node", "bun", "deno", "ruby", "perl", "bash", "sh", "zsh",
];

fn collect_segments(
    node: Node,
    src: &str,
    separator: Option<&'static str>,
    out: &mut Vec<ShellSegment>,
) {
    match node.kind() {
        "comment" | "heredoc_body" => {}
        "program" | "list" | "ERROR" => {
            let mut separator = separator;
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                match child.kind() {
                    "&&" => separator = Some("&&"),
                    "||" => separator = Some("||"),
                    _ if child.is_named() => {
                        collect_segments(child, src, separator, out);
                        separator = Some(";");
                    }
                    _ => {}
                }
            }
        }
        _ => {
            let mut stages = Vec::new();
            collect_stages(node, src, &mut stages);
            if !stages.is_empty() {
                out.push(ShellSegment { separator, stages });
            }
        }
    }
}

fn collect_stages(node: Node, src: &str, out: &mut Vec<ShellStage>) {
    match node.kind() {
        "pipeline" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                collect_stages(child, src, out);
            }
        }
        "redirected_statement" => {
            let Some(body) = node.child_by_field_name("body") else {
                out.push(opaque_stage(node, node, src));
                return;
            };
            let first = out.len();
            collect_stages(body, src, out);
            let mut trailing = Vec::new();
            let mut cursor = node.walk();
            for redirect in node.children_by_field_name("redirect", &mut cursor) {
                let mut inner = redirect.walk();
                for child in redirect.named_children(&mut inner) {
                    match child.kind() {
                        "heredoc_body" => {
                            let body = &src[child.byte_range()];
                            let lines = body
                                .trim_end_matches(['\t', ' '])
                                .trim_matches('\n')
                                .lines()
                                .count();
                            if let Some(stage) = out.get_mut(first) {
                                stage.heredoc_lines = Some(lines);
                            }
                        }
                        "pipeline" | "command" => collect_stages(child, src, &mut trailing),
                        _ => {}
                    }
                }
            }
            if let Some(stage) = out.get_mut(first) {
                stage.text = node.byte_range();
                stage.writes_file |= any_node(node, &|n| writes_to_file(n, src));
                stage.opaque |= any_node(node, &|n| SUBSTITUTIONS.contains(&n.kind()));
            }
            out.extend(trailing);
        }
        "command" => {
            let Some(name) = node.child_by_field_name("name") else {
                return;
            };
            let mut cursor = node.walk();
            let args: Vec<Node> = node
                .children_by_field_name("argument", &mut cursor)
                .collect();
            let base = src[name.byte_range()]
                .rsplit('/')
                .next()
                .unwrap_or_default();
            let script = args
                .first()
                .filter(|arg| {
                    let text = &src[arg.byte_range()];
                    INTERPRETERS.contains(&base) && !text.starts_with('-') && text.contains('.')
                })
                .map(Node::byte_range);
            out.push(ShellStage {
                name: name.byte_range(),
                has_args: args.len() > usize::from(script.is_some()),
                script,
                text: node.byte_range(),
                heredoc_lines: None,
                writes_file: any_node(node, &|n| writes_to_file(n, src)),
                opaque: any_node(node, &|n| SUBSTITUTIONS.contains(&n.kind())),
            });
        }
        _ => {
            if let Some(first) = node.child(0) {
                out.push(opaque_stage(node, first, src));
            }
        }
    }
}

/// A stage whose contents cannot be summarized, named after `name`.
fn opaque_stage(node: Node, name: Node, src: &str) -> ShellStage {
    ShellStage {
        name: name.byte_range(),
        script: None,
        text: node.byte_range(),
        has_args: node.child_count() > 1,
        heredoc_lines: None,
        writes_file: any_node(node, &|n| writes_to_file(n, src)),
        opaque: true,
    }
}

fn writes_to_file(redirect: Node, src: &str) -> bool {
    let op = src[redirect.byte_range()].trim_start_matches(char::is_numeric);
    let dest = redirect
        .child_by_field_name("destination")
        .map(|dest| &src[dest.byte_range()]);
    let duplicates_fd = op.starts_with(">&")
        && dest.is_some_and(|dest| dest == "-" || dest.chars().all(|c| c.is_ascii_digit()));
    redirect.kind() == "file_redirect"
        && (op.starts_with('>') || op.starts_with("&>"))
        && !duplicates_fd
        && dest != Some("/dev/null")
}

fn any_node(node: Node, matches: &dyn Fn(Node) -> bool) -> bool {
    let mut cursor = node.walk();
    matches(node)
        || node
            .children(&mut cursor)
            .any(|child| any_node(child, matches))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// One string per segment: `<sep> stage | stage`, each stage written as
    /// its name followed by its flags, e.g. `cat[heredoc=3,writes]`.
    fn shape(src: &str) -> Vec<String> {
        summarize(src)
            .expect("summary")
            .into_iter()
            .map(|segment| {
                let stages: Vec<String> = segment
                    .stages
                    .iter()
                    .map(|stage| {
                        let mut flags = Vec::new();
                        if let Some(script) = &stage.script {
                            flags.push(format!("script={}", &src[script.clone()]));
                        }
                        if stage.has_args {
                            flags.push("args".to_string());
                        }
                        if let Some(lines) = stage.heredoc_lines {
                            flags.push(format!("heredoc={lines}"));
                        }
                        if stage.writes_file {
                            flags.push("writes".to_string());
                        }
                        if stage.opaque {
                            flags.push("opaque".to_string());
                        }
                        format!("{}[{}]", &src[stage.name.clone()], flags.join(","))
                    })
                    .collect();
                format!(
                    "{} {}",
                    segment.separator.unwrap_or("^"),
                    stages.join(" | ")
                )
            })
            .collect()
    }

    #[test]
    fn splits_chains_and_pipelines() {
        assert_eq!(
            shape("FOO=1 cargo test -p x | grep ok && echo done || true; ls"),
            vec![
                "^ cargo[args] | grep[args]",
                "&& echo[args]",
                "|| true[]",
                "; ls[]",
            ]
        );
    }

    #[test]
    fn newlines_and_continuations() {
        assert_eq!(
            shape("cd /repo \\\n  && make \\\n    all\nwc -l out.txt"),
            vec!["^ cd[args]", "&& make[args]", "; wc[args]"]
        );
    }

    #[test]
    fn interpreter_script_and_heredoc() {
        assert_eq!(
            shape("python3 tools/gen.py --out x"),
            vec!["^ python3[script=tools/gen.py,args]"]
        );
        assert_eq!(
            shape("python3 - <<'EOF'\nimport sys\nprint(1)\nEOF"),
            vec!["^ python3[heredoc=2]"]
        );
        assert_eq!(
            shape("cat <<EOF > notes.txt\na\nb\nc\nEOF"),
            vec!["^ cat[heredoc=3,writes]"]
        );
    }

    #[test]
    fn dash_heredoc_keeps_trailing_pipeline() {
        assert_eq!(
            shape("cat <<-EOF | wc -l\n\ta\n\tEOF"),
            vec!["^ cat[heredoc=1] | wc[args]"]
        );
    }

    #[test]
    fn file_redirects_are_writes() {
        assert_eq!(
            shape(
                "rg foo 2>/dev/null > out.txt; grep x f >/dev/null 2>&1; ls &> log; ls >&log; >o cat f; cat a >> b"
            ),
            vec![
                "^ rg[args,writes]",
                "; grep[args]",
                "; ls[writes]",
                "; ls[writes]",
                "; cat[args,writes]",
                "; cat[args,writes]",
            ]
        );
    }

    #[test]
    fn substitutions_and_compound_statements_are_opaque() {
        assert_eq!(
            shape("ls $(rm -rf x); cat <(curl -s x); (cd x && ls); cat a; > b"),
            vec![
                "^ ls[args,opaque]",
                "; cat[args,opaque]",
                "; ([args,opaque]",
                "; cat[args]",
                "; > b[writes,opaque]",
            ]
        );
    }

    #[test]
    fn broken_input_keeps_recovered_commands_as_opaque() {
        assert_eq!(shape("echo \"unclosed && ls | "), vec!["^ echo[opaque]"]);
        assert_eq!(shape("cat <<EOF\nunterminated")[0], "^ cat[opaque]");
        assert_eq!(summarize(""), None);
    }
}
