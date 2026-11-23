//! How `tool_output_max_lines` resolves across `-c`, the active project, and `[tui]`.

use nori_config::NoriConfig;
use nori_config::NoriConfigOverrides;
use pretty_assertions::assert_eq;
use std::path::Path;
use std::path::PathBuf;
use tempfile::TempDir;

fn project_dir() -> std::io::Result<(TempDir, PathBuf)> {
    let dir = TempDir::new()?;
    let path = std::fs::canonicalize(dir.path())?;
    Ok((dir, path))
}

fn max_lines_for(
    config_toml: &str,
    cwd: &Path,
    raw_overrides: &[(&str, &str)],
) -> anyhow::Result<Option<i32>> {
    let home = TempDir::new()?;
    let config_path = home.path().join("config.toml");
    std::fs::write(&config_path, config_toml)?;
    let raw_overrides = raw_overrides
        .iter()
        .map(|(path, value)| {
            let table: toml::Table = toml::from_str(&format!("v = {value}"))?;
            Ok(((*path).to_string(), table["v"].clone()))
        })
        .collect::<anyhow::Result<_>>()?;
    Ok(NoriConfig::load_from_path_with_overrides(
        &config_path,
        NoriConfigOverrides {
            cwd: Some(cwd.to_path_buf()),
            raw_overrides,
            ..NoriConfigOverrides::default()
        },
    )?
    .tool_output_max_lines)
}

fn project_table(project: &Path, body: &str) -> String {
    format!("[projects.{:?}]\n{body}\n", project.display().to_string())
}

#[test]
fn output_is_uncapped_when_nothing_is_set() {
    let (_dir, project) = project_dir().expect("create project dir");
    assert_eq!(max_lines_for("", &project, &[]).expect("load config"), None);
}

#[test]
fn global_setting_caps_output() {
    let (_dir, project) = project_dir().expect("create project dir");
    assert_eq!(
        max_lines_for("[tui]\ntool_output_max_lines = 40\n", &project, &[]).expect("load config"),
        Some(40)
    );
}

#[test]
fn project_setting_beats_global_setting() {
    let (_dir, project) = project_dir().expect("create project dir");
    let config = format!(
        "[tui]\ntool_output_max_lines = 40\n\n{}",
        project_table(&project, "tool_output_max_lines = 12")
    );
    assert_eq!(
        max_lines_for(&config, &project, &[]).expect("load config"),
        Some(12)
    );
}

#[test]
fn project_zero_turns_off_a_global_cap() {
    let (_dir, project) = project_dir().expect("create project dir");
    let config = format!(
        "[tui]\ntool_output_max_lines = 40\n\n{}",
        project_table(&project, "tool_output_max_lines = 0")
    );
    assert_eq!(
        max_lines_for(&config, &project, &[]).expect("load config"),
        None
    );
}

#[test]
fn other_projects_keep_the_global_cap() {
    let (_dir, project) = project_dir().expect("create project dir");
    let (_other_dir, other) = project_dir().expect("create project dir");
    let config = format!(
        "[tui]\ntool_output_max_lines = 40\n\n{}",
        project_table(&other, "tool_output_max_lines = 0")
    );
    assert_eq!(
        max_lines_for(&config, &project, &[]).expect("load config"),
        Some(40)
    );
}

#[test]
fn command_line_beats_project_setting() {
    let (_dir, project) = project_dir().expect("create project dir");
    let config = project_table(&project, "tool_output_max_lines = 12");
    assert_eq!(
        max_lines_for(&config, &project, &[("tui.tool_output_max_lines", "5")])
            .expect("load config"),
        Some(5)
    );
    assert_eq!(
        max_lines_for(
            &config,
            &project,
            &[("tui", "{ tool_output_max_lines = 0 }")]
        )
        .expect("load config"),
        None
    );
}

#[test]
fn dotted_command_line_zero_turns_off_a_project_cap() {
    let (_dir, project) = project_dir().expect("create project dir");
    let config = project_table(&project, "tool_output_max_lines = 12");
    assert_eq!(
        max_lines_for(&config, &project, &[("tui.tool_output_max_lines", "0")])
            .expect("load config"),
        None
    );
}

#[test]
fn subdirectory_of_a_repository_uses_the_repository_cap() {
    let (_dir, project) = project_dir().expect("create project dir");
    let status = std::process::Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&project)
        .status()
        .expect("run git init");
    assert!(status.success());
    let nested = project.join("crates").join("widgets");
    std::fs::create_dir_all(&nested).expect("create nested dir");
    let config = project_table(&project, "tool_output_max_lines = 12");
    assert_eq!(
        max_lines_for(&config, &nested, &[]).expect("load config"),
        Some(12)
    );
}

#[test]
fn later_tui_table_without_the_key_drops_an_earlier_command_line_cap() {
    let (_dir, project) = project_dir().expect("create project dir");
    let config = project_table(&project, "tool_output_max_lines = 12");
    assert_eq!(
        max_lines_for(
            &config,
            &project,
            &[
                ("tui.tool_output_max_lines", "5"),
                ("tui", "{ animations = false }"),
            ]
        )
        .expect("load config"),
        Some(12)
    );
}

#[test]
fn negative_values_mean_uncapped() {
    let (_dir, project) = project_dir().expect("create project dir");
    assert_eq!(
        max_lines_for("[tui]\ntool_output_max_lines = -3\n", &project, &[]).expect("load config"),
        None
    );
}

#[test]
fn tool_output_mode_override_does_not_unlock_project_cap_override() {
    let (_dir, project) = project_dir().expect("create project dir");
    let config = format!(
        "[tui]\ntool_output_max_lines = 40\n\n{}",
        project_table(&project, "tool_output_max_lines = 12")
    );
    assert_eq!(
        max_lines_for(&config, &project, &[("tui.tool_output", "\"full\"")]).expect("load config"),
        Some(12)
    );
}
