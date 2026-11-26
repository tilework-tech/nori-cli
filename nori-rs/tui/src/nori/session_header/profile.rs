//! Opt-in measurements of the real instruction discovery and tokenizer path.

use super::*;
use pretty_assertions::assert_eq;
use std::time::Instant;

#[test]
#[allow(clippy::print_stdout)] // Opt-in profiler also emits its JSONL to the invoking terminal.
#[ignore = "writes profiling artifacts; run with --ignored --nocapture in a fresh process"]
fn profile_context_instruction_inventory() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let cwd = root.path().join("project");
    std::fs::create_dir_all(cwd.join(".claude"))?;
    let contents = "Use the component library. Keep interactions responsive.\n".repeat(100);
    for name in ["CLAUDE.md", "CLAUDE.local.md", ".claude/CLAUDE.md"] {
        std::fs::write(cwd.join(name), &contents)?;
    }
    let output = std::env::var_os("NORI_CONTEXT_PROFILE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.path().join("profile"));
    std::fs::create_dir_all(&output)?;
    let mut records = Vec::new();
    let mut expected = None;
    for iteration in 0..6 {
        let started = Instant::now();
        let files = discover_all_instruction_files_with_paths(
            &cwd,
            Some(AgentKindSimple::Claude),
            None,
            None,
        );
        let elapsed = started.elapsed();
        let counts: Vec<_> = files
            .iter()
            .filter(|file| file.active)
            .filter_map(|file| file.token_count.as_ref().map(|count| count.count))
            .collect();
        assert_eq!(counts.len(), 3);
        assert!(counts.iter().all(|count| *count > 0));
        if let Some(expected) = &expected {
            assert_eq!(&counts, expected);
        } else {
            expected = Some(counts.clone());
        }
        records.push(serde_json::json!({
            "stage": "instruction_inventory",
            "iteration": iteration,
            "temperature": if iteration == 0 { "cold" } else { "warm" },
            "files": counts.len(),
            "bytes": contents.len() * counts.len(),
            "tokens": counts.iter().sum::<i64>(),
            "elapsed_us": elapsed.as_micros(),
        }));
    }
    // Reusing a tokenizer must still read changed instruction contents.
    std::fs::write(cwd.join("CLAUDE.md"), contents.repeat(2))?;
    let changed =
        discover_all_instruction_files_with_paths(&cwd, Some(AgentKindSimple::Claude), None, None);
    let changed_total: i64 = changed
        .iter()
        .filter_map(|file| file.token_count.as_ref().map(|count| count.count))
        .sum();
    assert!(changed_total > expected.unwrap_or_default().iter().sum());
    let lines = records
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()?
        .join("\n");
    std::fs::write(
        output.join("instruction-inventory.jsonl"),
        format!("{lines}\n"),
    )?;
    println!("{lines}");
    if std::env::var_os("NORI_CONTEXT_ENFORCE_BUDGET").is_some() {
        for record in &records[1..] {
            assert!(
                record["elapsed_us"].as_i64().unwrap_or(i64::MAX) < 250_000,
                "repeated instruction discovery exceeded the 250ms interactive budget: {record}"
            );
        }
    }
    Ok(())
}
