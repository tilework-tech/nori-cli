use crate::system_info::read_active_skillset;
use std::fs;
use tempfile::TempDir;

#[test]
fn active_skillset_discovery_skips_config_without_active_skillset() {
    // Create a temp directory structure:
    // /tmp/xxx/
    //   .nori-config.json  (with active skillset)
    //   subdir/
    //     nested/  <- cwd
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    // Create nested directory structure
    let nested = root.join("subdir/nested");
    fs::create_dir_all(&nested).expect("create nested dirs");

    // Create a modern .nori-config.json at the root.
    let config_content = r#"{
        "activeSkillset": "test-skillset"
    }"#;
    fs::write(root.join(".nori-config.json"), config_content).expect("write config");

    // A nearer config without activeSkillset must not shadow the ancestor.
    fs::write(root.join("subdir/.nori-config.json"), r#"{"version":1}"#)
        .expect("write nearer config");

    // Discover the active skillset from the nested working directory.
    let skillset = read_active_skillset(&nested);

    assert_eq!(
        skillset,
        Some("test-skillset".to_string()),
        "Should find activeSkillset in ancestor .nori-config.json"
    );
}

#[test]
fn read_active_skillset_returns_none_when_no_config() {
    let tmp = TempDir::new().expect("tempdir");
    let skillset = read_active_skillset(tmp.path());
    assert_eq!(
        skillset, None,
        "Should return None when no config file exists"
    );
}
