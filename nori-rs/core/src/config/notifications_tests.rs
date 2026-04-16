use super::*;

#[test]
fn test_tui_terminal_notifications_defaults_to_true() {
    let toml = r#"
        [tui]
    "#;
    let parsed: toml::Value = toml::from_str(toml).expect("parse toml");
    let tui: Tui = parsed
        .get("tui")
        .unwrap()
        .clone()
        .try_into()
        .expect("deserialize tui");
    assert!(tui.terminal_notifications);
}

#[test]
fn test_tui_terminal_notifications_disabled() {
    let toml = r#"
        [tui]
        terminal_notifications = false
    "#;
    let parsed: toml::Value = toml::from_str(toml).expect("parse toml");
    let tui: Tui = parsed
        .get("tui")
        .unwrap()
        .clone()
        .try_into()
        .expect("deserialize tui");
    assert!(!tui.terminal_notifications);
}
