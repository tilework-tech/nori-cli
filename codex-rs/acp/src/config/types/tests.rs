use super::*;

#[test]
fn test_approval_policy_deserialize() {
    #[derive(Deserialize)]
    struct Wrapper {
        policy: ApprovalPolicy,
    }

    let w: Wrapper = toml::from_str(r#"policy = "always""#).unwrap();
    assert_eq!(w.policy, ApprovalPolicy::Always);

    let w: Wrapper = toml::from_str(r#"policy = "on-request""#).unwrap();
    assert_eq!(w.policy, ApprovalPolicy::OnRequest);

    let w: Wrapper = toml::from_str(r#"policy = "never""#).unwrap();
    assert_eq!(w.policy, ApprovalPolicy::Never);
}

#[test]
fn test_mcp_server_resolve_stdio() {
    let toml = McpServerConfigToml {
        command: Some("my-tool".to_string()),
        args: Some(vec!["--verbose".to_string()]),
        enabled: true,
        ..Default::default()
    };

    let config = toml.resolve().unwrap();
    assert!(matches!(
        config.transport,
        McpServerTransportConfig::Stdio { .. }
    ));
    assert!(config.enabled);
}

#[test]
fn test_mcp_server_resolve_http() {
    let toml = McpServerConfigToml {
        url: Some("https://example.com/mcp".to_string()),
        bearer_token_env_var: Some("API_TOKEN".to_string()),
        enabled: true,
        ..Default::default()
    };

    let config = toml.resolve().unwrap();
    assert!(matches!(
        config.transport,
        McpServerTransportConfig::StreamableHttp { .. }
    ));
}

#[test]
fn test_mcp_server_resolve_error_both() {
    let toml = McpServerConfigToml {
        command: Some("my-tool".to_string()),
        url: Some("https://example.com/mcp".to_string()),
        ..Default::default()
    };

    assert!(toml.resolve().is_err());
}

#[test]
fn test_mcp_server_resolve_error_neither() {
    let toml = McpServerConfigToml::default();
    assert!(toml.resolve().is_err());
}

#[test]
fn test_history_persistence_deserialize() {
    #[derive(Deserialize)]
    struct Wrapper {
        persistence: HistoryPersistence,
    }

    let w: Wrapper = toml::from_str(r#"persistence = "save-all""#).unwrap();
    assert_eq!(w.persistence, HistoryPersistence::SaveAll);

    let w: Wrapper = toml::from_str(r#"persistence = "none""#).unwrap();
    assert_eq!(w.persistence, HistoryPersistence::None);
}

#[test]
fn test_history_persistence_default() {
    assert_eq!(HistoryPersistence::default(), HistoryPersistence::SaveAll);
}

#[test]
fn test_notify_after_idle_deserialize_all_variants() {
    #[derive(Deserialize)]
    struct Wrapper {
        value: NotifyAfterIdle,
    }

    let w: Wrapper = toml::from_str(r#"value = "5s""#).unwrap();
    assert_eq!(w.value, NotifyAfterIdle::FiveSeconds);

    let w: Wrapper = toml::from_str(r#"value = "10s""#).unwrap();
    assert_eq!(w.value, NotifyAfterIdle::TenSeconds);

    let w: Wrapper = toml::from_str(r#"value = "30s""#).unwrap();
    assert_eq!(w.value, NotifyAfterIdle::ThirtySeconds);

    let w: Wrapper = toml::from_str(r#"value = "60s""#).unwrap();
    assert_eq!(w.value, NotifyAfterIdle::SixtySeconds);

    let w: Wrapper = toml::from_str(r#"value = "disabled""#).unwrap();
    assert_eq!(w.value, NotifyAfterIdle::Disabled);
}

#[test]
fn test_notify_after_idle_default() {
    assert_eq!(NotifyAfterIdle::default(), NotifyAfterIdle::FiveSeconds);
}

#[test]
fn test_notify_after_idle_as_duration() {
    assert_eq!(
        NotifyAfterIdle::FiveSeconds.as_duration(),
        Some(Duration::from_secs(5))
    );
    assert_eq!(
        NotifyAfterIdle::TenSeconds.as_duration(),
        Some(Duration::from_secs(10))
    );
    assert_eq!(
        NotifyAfterIdle::ThirtySeconds.as_duration(),
        Some(Duration::from_secs(30))
    );
    assert_eq!(
        NotifyAfterIdle::SixtySeconds.as_duration(),
        Some(Duration::from_secs(60))
    );
    assert_eq!(NotifyAfterIdle::Disabled.as_duration(), None);
}

#[test]
fn test_notify_after_idle_display_name() {
    assert_eq!(NotifyAfterIdle::FiveSeconds.display_name(), "5 seconds");
    assert_eq!(NotifyAfterIdle::TenSeconds.display_name(), "10 seconds");
    assert_eq!(NotifyAfterIdle::ThirtySeconds.display_name(), "30 seconds");
    assert_eq!(NotifyAfterIdle::SixtySeconds.display_name(), "1 minute");
    assert_eq!(NotifyAfterIdle::Disabled.display_name(), "Disabled");
}

#[test]
fn test_notify_after_idle_toml_value() {
    assert_eq!(NotifyAfterIdle::FiveSeconds.toml_value(), "5s");
    assert_eq!(NotifyAfterIdle::TenSeconds.toml_value(), "10s");
    assert_eq!(NotifyAfterIdle::ThirtySeconds.toml_value(), "30s");
    assert_eq!(NotifyAfterIdle::SixtySeconds.toml_value(), "60s");
    assert_eq!(NotifyAfterIdle::Disabled.toml_value(), "disabled");
}

#[test]
fn test_notify_after_idle_all_variants() {
    let variants = NotifyAfterIdle::all_variants();
    assert_eq!(variants.len(), 5);
    assert_eq!(variants[0], NotifyAfterIdle::FiveSeconds);
    assert_eq!(variants[4], NotifyAfterIdle::Disabled);
}

#[test]
fn test_tui_config_toml_with_notify_after_idle() {
    let config: TuiConfigToml = toml::from_str(
        r#"
notify_after_idle = "30s"
"#,
    )
    .unwrap();
    assert_eq!(
        config.notify_after_idle,
        Some(NotifyAfterIdle::ThirtySeconds)
    );
}

#[test]
fn test_tui_config_toml_without_notify_after_idle() {
    let config: TuiConfigToml = toml::from_str("").unwrap();
    assert_eq!(config.notify_after_idle, None);
}

// ========================================================================
// Hotkey Configuration Tests
// ========================================================================

#[test]
fn test_hotkey_binding_from_str_ctrl_t() {
    let binding = HotkeyBinding::from_str("ctrl+t");
    assert_eq!(binding.as_str(), "ctrl+t");
    assert!(!binding.is_none());
}

#[test]
fn test_hotkey_binding_from_str_none() {
    let binding = HotkeyBinding::from_str("none");
    assert!(binding.is_none());
    assert_eq!(binding.as_str(), "none");
}

#[test]
fn test_hotkey_binding_from_str_normalizes_case() {
    let binding = HotkeyBinding::from_str("Ctrl+T");
    assert_eq!(binding.as_str(), "ctrl+t");
}

#[test]
fn test_hotkey_binding_display_name() {
    let binding = HotkeyBinding::from_str("ctrl+t");
    assert_eq!(binding.display_name(), "ctrl + t");

    let unbound = HotkeyBinding::none();
    assert_eq!(unbound.display_name(), "unbound");
}

#[test]
fn test_hotkey_binding_toml_value() {
    let binding = HotkeyBinding::from_str("ctrl+g");
    assert_eq!(binding.toml_value(), "ctrl+g");

    let unbound = HotkeyBinding::none();
    assert_eq!(unbound.toml_value(), "none");
}

#[test]
fn test_hotkey_binding_serde_roundtrip() {
    #[derive(Serialize, Deserialize)]
    struct Wrapper {
        key: HotkeyBinding,
    }

    let w = Wrapper {
        key: HotkeyBinding::from_str("ctrl+t"),
    };
    let toml_str = toml::to_string(&w).unwrap();
    let parsed: Wrapper = toml::from_str(&toml_str).unwrap();
    assert_eq!(parsed.key, HotkeyBinding::from_str("ctrl+t"));
}

#[test]
fn test_hotkey_binding_serde_none_roundtrip() {
    #[derive(Serialize, Deserialize)]
    struct Wrapper {
        key: HotkeyBinding,
    }

    let w = Wrapper {
        key: HotkeyBinding::none(),
    };
    let toml_str = toml::to_string(&w).unwrap();
    let parsed: Wrapper = toml::from_str(&toml_str).unwrap();
    assert!(parsed.key.is_none());
}

#[test]
fn test_hotkey_binding_deserialize_from_toml_string() {
    #[derive(Deserialize)]
    struct Wrapper {
        key: HotkeyBinding,
    }

    let w: Wrapper = toml::from_str(r#"key = "alt+x""#).unwrap();
    assert_eq!(w.key.as_str(), "alt+x");

    let w: Wrapper = toml::from_str(r#"key = "none""#).unwrap();
    assert!(w.key.is_none());
}

#[test]
fn test_hotkey_action_display_names() {
    assert_eq!(
        HotkeyAction::OpenTranscript.display_name(),
        "Open Transcript"
    );
    assert_eq!(HotkeyAction::OpenEditor.display_name(), "Open Editor");
}

#[test]
fn test_hotkey_action_toml_keys() {
    assert_eq!(HotkeyAction::OpenTranscript.toml_key(), "open_transcript");
    assert_eq!(HotkeyAction::OpenEditor.toml_key(), "open_editor");
}

#[test]
fn test_hotkey_action_default_bindings() {
    assert_eq!(HotkeyAction::OpenTranscript.default_binding(), "ctrl+t");
    assert_eq!(HotkeyAction::OpenEditor.default_binding(), "ctrl+g");
}

#[test]
fn test_hotkey_action_all_actions() {
    let actions = HotkeyAction::all_actions();
    assert_eq!(actions.len(), 15);
    assert_eq!(actions[0], HotkeyAction::OpenTranscript);
    assert_eq!(actions[1], HotkeyAction::OpenEditor);
    assert_eq!(actions[2], HotkeyAction::MoveBackwardChar);
    assert_eq!(actions[3], HotkeyAction::MoveForwardChar);
    assert_eq!(actions[4], HotkeyAction::MoveBeginningOfLine);
    assert_eq!(actions[5], HotkeyAction::MoveEndOfLine);
    assert_eq!(actions[6], HotkeyAction::MoveBackwardWord);
    assert_eq!(actions[7], HotkeyAction::MoveForwardWord);
    assert_eq!(actions[8], HotkeyAction::DeleteBackwardChar);
    assert_eq!(actions[9], HotkeyAction::DeleteForwardChar);
    assert_eq!(actions[10], HotkeyAction::DeleteBackwardWord);
    assert_eq!(actions[11], HotkeyAction::KillToEndOfLine);
    assert_eq!(actions[12], HotkeyAction::KillToBeginningOfLine);
    assert_eq!(actions[13], HotkeyAction::Yank);
    assert_eq!(actions[14], HotkeyAction::HistorySearch);
}

#[test]
fn test_hotkey_config_default_uses_standard_bindings() {
    let config = HotkeyConfig::default();
    assert_eq!(config.open_transcript, HotkeyBinding::from_str("ctrl+t"));
    assert_eq!(config.open_editor, HotkeyBinding::from_str("ctrl+g"));
}

#[test]
fn test_hotkey_config_from_toml_uses_defaults_when_empty() {
    let toml = HotkeyConfigToml::default();
    let config = HotkeyConfig::from_toml(&toml);
    assert_eq!(config.open_transcript, HotkeyBinding::from_str("ctrl+t"));
    assert_eq!(config.open_editor, HotkeyBinding::from_str("ctrl+g"));
}

#[test]
fn test_hotkey_config_from_toml_uses_custom_bindings() {
    let toml = HotkeyConfigToml {
        open_transcript: Some(HotkeyBinding::from_str("alt+t")),
        open_editor: Some(HotkeyBinding::from_str("ctrl+e")),
        ..Default::default()
    };
    let config = HotkeyConfig::from_toml(&toml);
    assert_eq!(config.open_transcript, HotkeyBinding::from_str("alt+t"));
    assert_eq!(config.open_editor, HotkeyBinding::from_str("ctrl+e"));
}

#[test]
fn test_hotkey_config_from_toml_partial_override() {
    let toml = HotkeyConfigToml {
        open_transcript: Some(HotkeyBinding::from_str("alt+t")),
        open_editor: None,
        ..Default::default()
    };
    let config = HotkeyConfig::from_toml(&toml);
    assert_eq!(config.open_transcript, HotkeyBinding::from_str("alt+t"));
    assert_eq!(config.open_editor, HotkeyBinding::from_str("ctrl+g")); // default
}

#[test]
fn test_hotkey_config_from_toml_unbind_action() {
    let toml = HotkeyConfigToml {
        open_transcript: Some(HotkeyBinding::none()),
        open_editor: None,
        ..Default::default()
    };
    let config = HotkeyConfig::from_toml(&toml);
    assert!(config.open_transcript.is_none());
    assert_eq!(config.open_editor, HotkeyBinding::from_str("ctrl+g"));
}

#[test]
fn test_hotkey_config_binding_for_action() {
    let config = HotkeyConfig::default();
    assert_eq!(
        config.binding_for(HotkeyAction::OpenTranscript),
        &HotkeyBinding::from_str("ctrl+t")
    );
    assert_eq!(
        config.binding_for(HotkeyAction::OpenEditor),
        &HotkeyBinding::from_str("ctrl+g")
    );
}

#[test]
fn test_hotkey_config_set_binding() {
    let mut config = HotkeyConfig::default();
    config.set_binding(HotkeyAction::OpenTranscript, HotkeyBinding::from_str("f1"));
    assert_eq!(config.open_transcript, HotkeyBinding::from_str("f1"));
}

#[test]
fn test_hotkey_config_all_bindings() {
    let config = HotkeyConfig::default();
    let bindings = config.all_bindings();
    assert_eq!(bindings.len(), 15);
    assert_eq!(bindings[0].0, HotkeyAction::OpenTranscript);
    assert_eq!(bindings[1].0, HotkeyAction::OpenEditor);
}

#[test]
fn test_tui_config_toml_with_hotkeys() {
    let config: TuiConfigToml = toml::from_str(
        r#"
[hotkeys]
open_transcript = "alt+t"
open_editor = "ctrl+e"
"#,
    )
    .unwrap();
    assert_eq!(
        config.hotkeys.open_transcript,
        Some(HotkeyBinding::from_str("alt+t"))
    );
    assert_eq!(
        config.hotkeys.open_editor,
        Some(HotkeyBinding::from_str("ctrl+e"))
    );
}

#[test]
fn test_tui_config_toml_without_hotkeys() {
    let config: TuiConfigToml = toml::from_str("").unwrap();
    assert!(config.hotkeys.open_transcript.is_none());
    assert!(config.hotkeys.open_editor.is_none());
}

#[test]
fn test_full_config_toml_with_hotkeys() {
    let config: NoriConfigToml = toml::from_str(
        r#"
model = "claude-code"

[tui]
vertical_footer = true

[tui.hotkeys]
open_transcript = "ctrl+y"
open_editor = "none"
"#,
    )
    .unwrap();
    assert_eq!(
        config.tui.hotkeys.open_transcript,
        Some(HotkeyBinding::from_str("ctrl+y"))
    );
    assert_eq!(config.tui.hotkeys.open_editor, Some(HotkeyBinding::none()));
}

// ========================================================================
// Editing Hotkey Tests
// ========================================================================

#[test]
fn test_editing_hotkey_action_display_names() {
    use pretty_assertions::assert_eq;
    assert_eq!(
        HotkeyAction::MoveBackwardChar.display_name(),
        "Move Backward Char"
    );
    assert_eq!(
        HotkeyAction::MoveForwardChar.display_name(),
        "Move Forward Char"
    );
    assert_eq!(
        HotkeyAction::MoveBeginningOfLine.display_name(),
        "Move to Line Start"
    );
    assert_eq!(
        HotkeyAction::MoveEndOfLine.display_name(),
        "Move to Line End"
    );
    assert_eq!(
        HotkeyAction::MoveBackwardWord.display_name(),
        "Move Backward Word"
    );
    assert_eq!(
        HotkeyAction::MoveForwardWord.display_name(),
        "Move Forward Word"
    );
    assert_eq!(
        HotkeyAction::DeleteBackwardChar.display_name(),
        "Delete Backward Char"
    );
    assert_eq!(
        HotkeyAction::DeleteForwardChar.display_name(),
        "Delete Forward Char"
    );
    assert_eq!(
        HotkeyAction::DeleteBackwardWord.display_name(),
        "Delete Backward Word"
    );
    assert_eq!(
        HotkeyAction::KillToEndOfLine.display_name(),
        "Kill to Line End"
    );
    assert_eq!(
        HotkeyAction::KillToBeginningOfLine.display_name(),
        "Kill to Line Start"
    );
    assert_eq!(HotkeyAction::Yank.display_name(), "Yank");
}

#[test]
fn test_editing_hotkey_action_toml_keys() {
    use pretty_assertions::assert_eq;
    assert_eq!(
        HotkeyAction::MoveBackwardChar.toml_key(),
        "move_backward_char"
    );
    assert_eq!(
        HotkeyAction::MoveForwardChar.toml_key(),
        "move_forward_char"
    );
    assert_eq!(
        HotkeyAction::MoveBeginningOfLine.toml_key(),
        "move_beginning_of_line"
    );
    assert_eq!(HotkeyAction::MoveEndOfLine.toml_key(), "move_end_of_line");
    assert_eq!(
        HotkeyAction::MoveBackwardWord.toml_key(),
        "move_backward_word"
    );
    assert_eq!(
        HotkeyAction::MoveForwardWord.toml_key(),
        "move_forward_word"
    );
    assert_eq!(
        HotkeyAction::DeleteBackwardChar.toml_key(),
        "delete_backward_char"
    );
    assert_eq!(
        HotkeyAction::DeleteForwardChar.toml_key(),
        "delete_forward_char"
    );
    assert_eq!(
        HotkeyAction::DeleteBackwardWord.toml_key(),
        "delete_backward_word"
    );
    assert_eq!(
        HotkeyAction::KillToEndOfLine.toml_key(),
        "kill_to_end_of_line"
    );
    assert_eq!(
        HotkeyAction::KillToBeginningOfLine.toml_key(),
        "kill_to_beginning_of_line"
    );
    assert_eq!(HotkeyAction::Yank.toml_key(), "yank");
}

#[test]
fn test_editing_hotkey_action_default_bindings() {
    use pretty_assertions::assert_eq;
    assert_eq!(HotkeyAction::MoveBackwardChar.default_binding(), "ctrl+b");
    assert_eq!(HotkeyAction::MoveForwardChar.default_binding(), "ctrl+f");
    assert_eq!(
        HotkeyAction::MoveBeginningOfLine.default_binding(),
        "ctrl+a"
    );
    assert_eq!(HotkeyAction::MoveEndOfLine.default_binding(), "ctrl+e");
    assert_eq!(HotkeyAction::MoveBackwardWord.default_binding(), "alt+b");
    assert_eq!(HotkeyAction::MoveForwardWord.default_binding(), "alt+f");
    assert_eq!(HotkeyAction::DeleteBackwardChar.default_binding(), "ctrl+h");
    assert_eq!(HotkeyAction::DeleteForwardChar.default_binding(), "ctrl+d");
    assert_eq!(HotkeyAction::DeleteBackwardWord.default_binding(), "ctrl+w");
    assert_eq!(HotkeyAction::KillToEndOfLine.default_binding(), "ctrl+k");
    assert_eq!(
        HotkeyAction::KillToBeginningOfLine.default_binding(),
        "ctrl+u"
    );
    assert_eq!(HotkeyAction::Yank.default_binding(), "ctrl+y");
}

#[test]
fn test_hotkey_config_default_includes_editing_bindings() {
    use pretty_assertions::assert_eq;
    let config = HotkeyConfig::default();
    assert_eq!(config.move_backward_char, HotkeyBinding::from_str("ctrl+b"));
    assert_eq!(config.move_forward_char, HotkeyBinding::from_str("ctrl+f"));
    assert_eq!(
        config.move_beginning_of_line,
        HotkeyBinding::from_str("ctrl+a")
    );
    assert_eq!(config.move_end_of_line, HotkeyBinding::from_str("ctrl+e"));
    assert_eq!(config.move_backward_word, HotkeyBinding::from_str("alt+b"));
    assert_eq!(config.move_forward_word, HotkeyBinding::from_str("alt+f"));
    assert_eq!(
        config.delete_backward_char,
        HotkeyBinding::from_str("ctrl+h")
    );
    assert_eq!(
        config.delete_forward_char,
        HotkeyBinding::from_str("ctrl+d")
    );
    assert_eq!(
        config.delete_backward_word,
        HotkeyBinding::from_str("ctrl+w")
    );
    assert_eq!(
        config.kill_to_end_of_line,
        HotkeyBinding::from_str("ctrl+k")
    );
    assert_eq!(
        config.kill_to_beginning_of_line,
        HotkeyBinding::from_str("ctrl+u")
    );
    assert_eq!(config.yank, HotkeyBinding::from_str("ctrl+y"));
}

#[test]
fn test_hotkey_config_from_toml_editing_overrides() {
    use pretty_assertions::assert_eq;
    let toml = HotkeyConfigToml {
        open_transcript: None,
        open_editor: None,
        move_backward_char: Some(HotkeyBinding::from_str("alt+left")),
        move_forward_char: Some(HotkeyBinding::from_str("alt+right")),
        move_beginning_of_line: None,
        move_end_of_line: None,
        move_backward_word: None,
        move_forward_word: None,
        delete_backward_char: None,
        delete_forward_char: None,
        delete_backward_word: None,
        kill_to_end_of_line: None,
        kill_to_beginning_of_line: None,
        yank: None,
        history_search: None,
    };
    let config = HotkeyConfig::from_toml(&toml);
    assert_eq!(
        config.move_backward_char,
        HotkeyBinding::from_str("alt+left")
    );
    assert_eq!(
        config.move_forward_char,
        HotkeyBinding::from_str("alt+right")
    );
    // Others should keep defaults
    assert_eq!(
        config.move_beginning_of_line,
        HotkeyBinding::from_str("ctrl+a")
    );
    assert_eq!(
        config.kill_to_end_of_line,
        HotkeyBinding::from_str("ctrl+k")
    );
}

#[test]
fn test_hotkey_config_from_toml_editing_unbind() {
    use pretty_assertions::assert_eq;
    let toml = HotkeyConfigToml {
        open_transcript: None,
        open_editor: None,
        move_backward_char: Some(HotkeyBinding::none()),
        move_forward_char: None,
        move_beginning_of_line: None,
        move_end_of_line: None,
        move_backward_word: None,
        move_forward_word: None,
        delete_backward_char: None,
        delete_forward_char: None,
        delete_backward_word: None,
        kill_to_end_of_line: None,
        kill_to_beginning_of_line: None,
        yank: None,
        history_search: None,
    };
    let config = HotkeyConfig::from_toml(&toml);
    assert!(config.move_backward_char.is_none());
    // Others should keep defaults
    assert_eq!(config.move_forward_char, HotkeyBinding::from_str("ctrl+f"));
}

#[test]
fn test_hotkey_config_all_bindings_includes_editing() {
    let config = HotkeyConfig::default();
    let bindings = config.all_bindings();
    assert_eq!(bindings.len(), 15);
    // First two are app-level actions
    assert_eq!(bindings[0].0, HotkeyAction::OpenTranscript);
    assert_eq!(bindings[1].0, HotkeyAction::OpenEditor);
    // Then editing actions
    assert_eq!(bindings[2].0, HotkeyAction::MoveBackwardChar);
    assert_eq!(bindings[13].0, HotkeyAction::Yank);
}

#[test]
fn test_tui_config_toml_with_editing_hotkeys() {
    let config: TuiConfigToml = toml::from_str(
        r#"
[hotkeys]
move_backward_char = "alt+left"
kill_to_end_of_line = "none"
"#,
    )
    .unwrap();
    assert_eq!(
        config.hotkeys.move_backward_char,
        Some(HotkeyBinding::from_str("alt+left"))
    );
    assert_eq!(
        config.hotkeys.kill_to_end_of_line,
        Some(HotkeyBinding::none())
    );
    // Unset fields should be None
    assert!(config.hotkeys.move_forward_char.is_none());
}

#[test]
fn test_hotkey_config_binding_for_editing_action() {
    use pretty_assertions::assert_eq;
    let config = HotkeyConfig::default();
    assert_eq!(
        config.binding_for(HotkeyAction::MoveBackwardChar),
        &HotkeyBinding::from_str("ctrl+b")
    );
    assert_eq!(
        config.binding_for(HotkeyAction::KillToEndOfLine),
        &HotkeyBinding::from_str("ctrl+k")
    );
    assert_eq!(
        config.binding_for(HotkeyAction::Yank),
        &HotkeyBinding::from_str("ctrl+y")
    );
}

// ========================================================================
// Script Timeout Configuration Tests
// ========================================================================

#[test]
fn test_script_timeout_parse_seconds() {
    let timeout = ScriptTimeout::from_str("30s");
    assert_eq!(timeout.as_duration(), Duration::from_secs(30));
}

#[test]
fn test_script_timeout_parse_minutes() {
    let timeout = ScriptTimeout::from_str("2m");
    assert_eq!(timeout.as_duration(), Duration::from_secs(120));
}

#[test]
fn test_script_timeout_parse_5m() {
    let timeout = ScriptTimeout::from_str("5m");
    assert_eq!(timeout.as_duration(), Duration::from_secs(300));
}

#[test]
fn test_script_timeout_default_is_30s() {
    let timeout = ScriptTimeout::default();
    assert_eq!(timeout.as_duration(), Duration::from_secs(30));
}

#[test]
fn test_script_timeout_display_name() {
    let timeout = ScriptTimeout::from_str("30s");
    assert_eq!(timeout.display_name(), "30s");

    let timeout = ScriptTimeout::from_str("2m");
    assert_eq!(timeout.display_name(), "2m");
}

#[test]
fn test_script_timeout_toml_value() {
    let timeout = ScriptTimeout::from_str("30s");
    assert_eq!(timeout.toml_value(), "30s");
}

#[test]
fn test_script_timeout_deserialize_from_toml() {
    #[derive(Deserialize)]
    struct Wrapper {
        timeout: ScriptTimeout,
    }

    let w: Wrapper = toml::from_str(r#"timeout = "30s""#).unwrap();
    assert_eq!(w.timeout.as_duration(), Duration::from_secs(30));

    let w: Wrapper = toml::from_str(r#"timeout = "2m""#).unwrap();
    assert_eq!(w.timeout.as_duration(), Duration::from_secs(120));
}

#[test]
fn test_script_timeout_in_tui_config_toml() {
    let config: TuiConfigToml = toml::from_str(
        r#"
script_timeout = "45s"
"#,
    )
    .unwrap();
    assert!(config.script_timeout.is_some());
    assert_eq!(
        config.script_timeout.unwrap().as_duration(),
        Duration::from_secs(45)
    );
}

#[test]
fn test_script_timeout_absent_from_tui_config_toml() {
    let config: TuiConfigToml = toml::from_str("").unwrap();
    assert!(config.script_timeout.is_none());
}

#[test]
fn test_script_timeout_in_nori_config() {
    let config = NoriConfig::default();
    assert_eq!(config.script_timeout.as_duration(), Duration::from_secs(30));
}

#[test]
fn test_full_config_toml_with_script_timeout() {
    let config: NoriConfigToml = toml::from_str(
        r#"
model = "claude-code"

[tui]
script_timeout = "2m"
"#,
    )
    .unwrap();
    assert!(config.tui.script_timeout.is_some());
    assert_eq!(
        config.tui.script_timeout.unwrap().as_duration(),
        Duration::from_secs(120)
    );
}

// ========================================================================
// Footer Segment Configuration Tests
// ========================================================================

#[test]
fn test_footer_segment_deserialize_all_variants() {
    use pretty_assertions::assert_eq;
    #[derive(Deserialize)]
    struct Wrapper {
        segment: FooterSegment,
    }

    let w: Wrapper = toml::from_str(r#"segment = "prompt_summary""#).unwrap();
    assert_eq!(w.segment, FooterSegment::PromptSummary);

    let w: Wrapper = toml::from_str(r#"segment = "vim_mode""#).unwrap();
    assert_eq!(w.segment, FooterSegment::VimMode);

    let w: Wrapper = toml::from_str(r#"segment = "git_branch""#).unwrap();
    assert_eq!(w.segment, FooterSegment::GitBranch);

    let w: Wrapper = toml::from_str(r#"segment = "worktree_name""#).unwrap();
    assert_eq!(w.segment, FooterSegment::WorktreeName);

    let w: Wrapper = toml::from_str(r#"segment = "git_stats""#).unwrap();
    assert_eq!(w.segment, FooterSegment::GitStats);

    let w: Wrapper = toml::from_str(r#"segment = "context""#).unwrap();
    assert_eq!(w.segment, FooterSegment::Context);

    let w: Wrapper = toml::from_str(r#"segment = "approval_mode""#).unwrap();
    assert_eq!(w.segment, FooterSegment::ApprovalMode);

    let w: Wrapper = toml::from_str(r#"segment = "nori_profile""#).unwrap();
    assert_eq!(w.segment, FooterSegment::NoriProfile);

    let w: Wrapper = toml::from_str(r#"segment = "nori_version""#).unwrap();
    assert_eq!(w.segment, FooterSegment::NoriVersion);

    let w: Wrapper = toml::from_str(r#"segment = "token_usage""#).unwrap();
    assert_eq!(w.segment, FooterSegment::TokenUsage);
}

#[test]
fn test_footer_segment_serialize() {
    // TOML doesn't support bare enums, so we test within a struct
    #[derive(Serialize)]
    struct Wrapper {
        segment: FooterSegment,
    }
    let w = Wrapper {
        segment: FooterSegment::PromptSummary,
    };
    assert!(toml::to_string(&w).unwrap().contains("prompt_summary"));

    let w = Wrapper {
        segment: FooterSegment::GitBranch,
    };
    assert!(toml::to_string(&w).unwrap().contains("git_branch"));
}

#[test]
fn test_footer_segment_display_name() {
    use pretty_assertions::assert_eq;
    assert_eq!(FooterSegment::PromptSummary.display_name(), "Task Summary");
    assert_eq!(FooterSegment::VimMode.display_name(), "Vim Mode");
    assert_eq!(FooterSegment::GitBranch.display_name(), "Git Branch");
    assert_eq!(FooterSegment::WorktreeName.display_name(), "Worktree Name");
    assert_eq!(FooterSegment::GitStats.display_name(), "Git Stats");
    assert_eq!(FooterSegment::Context.display_name(), "Context Window");
    assert_eq!(FooterSegment::ApprovalMode.display_name(), "Approvals");
    assert_eq!(FooterSegment::NoriProfile.display_name(), "Skillset");
    assert_eq!(
        FooterSegment::NoriVersion.display_name(),
        "Skillset Version"
    );
    assert_eq!(FooterSegment::TokenUsage.display_name(), "Token Usage");
}

#[test]
fn test_footer_segment_all_variants() {
    use pretty_assertions::assert_eq;
    assert_eq!(
        FooterSegment::all_variants(),
        &[
            FooterSegment::PromptSummary,
            FooterSegment::VimMode,
            FooterSegment::GitBranch,
            FooterSegment::WorktreeName,
            FooterSegment::GitStats,
            FooterSegment::Context,
            FooterSegment::ApprovalMode,
            FooterSegment::NoriProfile,
            FooterSegment::NoriVersion,
            FooterSegment::TokenUsage,
        ]
    );
}

#[test]
fn test_footer_segment_default_order() {
    use pretty_assertions::assert_eq;
    let order = FooterSegment::default_order();
    assert_eq!(order, FooterSegment::all_variants());
}

#[test]
fn test_footer_segment_config_default_all_enabled() {
    let config = FooterSegmentConfig::default();
    for segment in FooterSegment::all_variants() {
        assert!(
            config.is_enabled(*segment),
            "Segment {segment:?} should be enabled by default"
        );
    }
}

#[test]
fn test_footer_segment_config_disable_segment() {
    let mut config = FooterSegmentConfig::default();
    config.set_enabled(FooterSegment::GitBranch, false);
    assert!(!config.is_enabled(FooterSegment::GitBranch));
    assert!(config.is_enabled(FooterSegment::Context));
}

#[test]
fn test_footer_segment_config_from_toml_empty() {
    let toml = FooterSegmentConfigToml::default();
    let config = FooterSegmentConfig::from_toml(&toml);
    // All segments enabled by default
    for segment in FooterSegment::all_variants() {
        assert!(config.is_enabled(*segment));
    }
}

#[test]
fn test_footer_segment_config_from_toml_some_disabled() {
    let toml = FooterSegmentConfigToml {
        prompt_summary: Some(false),
        git_branch: Some(false),
        token_usage: Some(false),
        ..Default::default()
    };
    let config = FooterSegmentConfig::from_toml(&toml);
    assert!(!config.is_enabled(FooterSegment::PromptSummary));
    assert!(!config.is_enabled(FooterSegment::GitBranch));
    assert!(!config.is_enabled(FooterSegment::TokenUsage));
    assert!(config.is_enabled(FooterSegment::Context));
    assert!(config.is_enabled(FooterSegment::ApprovalMode));
}

#[test]
fn test_tui_config_toml_with_footer_segments() {
    let config: TuiConfigToml = toml::from_str(
        r#"
[footer_segments]
git_branch = false
token_usage = false
"#,
    )
    .unwrap();
    assert_eq!(config.footer_segments.git_branch, Some(false));
    assert_eq!(config.footer_segments.token_usage, Some(false));
    assert_eq!(config.footer_segments.context, None);
}

#[test]
fn test_full_config_toml_with_footer_segments() {
    let config: NoriConfigToml = toml::from_str(
        r#"
model = "claude-code"

[tui]
vertical_footer = true

[tui.footer_segments]
prompt_summary = false
vim_mode = false
nori_profile = true
"#,
    )
    .unwrap();
    assert_eq!(config.tui.footer_segments.prompt_summary, Some(false));
    assert_eq!(config.tui.footer_segments.vim_mode, Some(false));
    assert_eq!(config.tui.footer_segments.nori_profile, Some(true));
    assert_eq!(config.tui.footer_segments.git_branch, None);
}

// ========================================================================
// Agent Configuration Tests
// ========================================================================

#[test]
fn test_agent_config_toml_deserialize_npx_distribution() {
    let config: NoriConfigToml = toml::from_str(
        r#"
[[agents]]
name = "Claude Code"
slug = "claude-code"

[agents.distribution.npx]
package = "@zed-industries/claude-agent-acp"
"#,
    )
    .unwrap();
    assert_eq!(config.agents.len(), 1);
    assert_eq!(config.agents[0].name, "Claude Code");
    assert_eq!(config.agents[0].slug, "claude-code");
    assert!(config.agents[0].distribution.npx.is_some());
    assert_eq!(
        config.agents[0].distribution.npx.as_ref().unwrap().package,
        "@zed-industries/claude-agent-acp"
    );
}

#[test]
fn test_agent_config_toml_deserialize_bunx_distribution() {
    let config: NoriConfigToml = toml::from_str(
        r#"
[[agents]]
name = "Gemini"
slug = "gemini"

[agents.distribution.bunx]
package = "@google/gemini-cli"
args = ["--experimental-acp"]
"#,
    )
    .unwrap();
    assert_eq!(config.agents.len(), 1);
    assert_eq!(config.agents[0].name, "Gemini");
    let bunx = config.agents[0].distribution.bunx.as_ref().unwrap();
    assert_eq!(bunx.package, "@google/gemini-cli");
    assert_eq!(bunx.args, vec!["--experimental-acp"]);
}

#[test]
fn test_agent_config_toml_deserialize_uvx_distribution() {
    let config: NoriConfigToml = toml::from_str(
        r#"
[[agents]]
name = "Kimi"
slug = "kimi"

[agents.distribution.uvx]
package = "kimi-cli"
args = ["acp"]
"#,
    )
    .unwrap();
    assert_eq!(config.agents.len(), 1);
    assert_eq!(config.agents[0].slug, "kimi");
    let uvx = config.agents[0].distribution.uvx.as_ref().unwrap();
    assert_eq!(uvx.package, "kimi-cli");
    assert_eq!(uvx.args, vec!["acp"]);
}

#[test]
fn test_agent_config_toml_deserialize_pipx_distribution() {
    let config: NoriConfigToml = toml::from_str(
        r#"
[[agents]]
name = "Python Agent"
slug = "py-agent"

[agents.distribution.pipx]
package = "py-agent-cli"
"#,
    )
    .unwrap();
    assert_eq!(config.agents.len(), 1);
    let pipx = config.agents[0].distribution.pipx.as_ref().unwrap();
    assert_eq!(pipx.package, "py-agent-cli");
}

#[test]
fn test_agent_config_toml_deserialize_local_distribution() {
    let config: NoriConfigToml = toml::from_str(
        r#"
[[agents]]
name = "My Local Agent"
slug = "my-agent"

[agents.distribution.local]
command = "/usr/local/bin/my-agent"
args = ["--acp"]

[agents.distribution.local.env]
MY_VAR = "value"
"#,
    )
    .unwrap();
    assert_eq!(config.agents.len(), 1);
    let local = config.agents[0].distribution.local.as_ref().unwrap();
    assert_eq!(local.command, "/usr/local/bin/my-agent");
    assert_eq!(local.args, vec!["--acp"]);
    assert_eq!(local.env.get("MY_VAR").unwrap(), "value");
}

#[test]
fn test_agent_config_toml_deserialize_multiple_agents() {
    let config: NoriConfigToml = toml::from_str(
        r#"
[[agents]]
name = "Claude Code"
slug = "claude-code"

[agents.distribution.npx]
package = "@zed-industries/claude-agent-acp"

[[agents]]
name = "Kimi"
slug = "kimi"

[agents.distribution.uvx]
package = "kimi-cli"
args = ["acp"]
"#,
    )
    .unwrap();
    assert_eq!(config.agents.len(), 2);
    assert_eq!(config.agents[0].slug, "claude-code");
    assert_eq!(config.agents[1].slug, "kimi");
}

#[test]
fn test_agent_config_toml_default_has_no_agents() {
    let config: NoriConfigToml = toml::from_str("").unwrap();
    assert!(config.agents.is_empty());
}

#[test]
fn test_agent_config_toml_optional_fields() {
    let config: NoriConfigToml = toml::from_str(
        r#"
[[agents]]
name = "Custom Agent"
slug = "custom"
auth_hint = "Set CUSTOM_API_KEY"
context_window_size = 500000
transcript_base_dir = ".custom/sessions"

[agents.distribution.local]
command = "/usr/bin/custom-agent"
"#,
    )
    .unwrap();
    assert_eq!(
        config.agents[0].auth_hint.as_deref(),
        Some("Set CUSTOM_API_KEY")
    );
    assert_eq!(config.agents[0].context_window_size, Some(500000));
    assert_eq!(
        config.agents[0].transcript_base_dir.as_deref(),
        Some(".custom/sessions")
    );
}

#[test]
fn test_agent_distribution_resolve_exactly_one_required() {
    // No distribution set
    let dist = AgentDistributionToml::default();
    assert!(dist.resolve().is_err());

    // Exactly one set - should pass
    let dist = AgentDistributionToml {
        npx: Some(PackageDistribution {
            package: "some-pkg".to_string(),
            args: vec![],
        }),
        ..Default::default()
    };
    assert!(dist.resolve().is_ok());
}

#[test]
fn test_agent_distribution_resolve_rejects_multiple() {
    let dist = AgentDistributionToml {
        npx: Some(PackageDistribution {
            package: "pkg-a".to_string(),
            args: vec![],
        }),
        bunx: Some(PackageDistribution {
            package: "pkg-b".to_string(),
            args: vec![],
        }),
        ..Default::default()
    };
    assert!(dist.resolve().is_err());
}

#[test]
fn test_agent_distribution_resolve_npx() {
    let dist = AgentDistributionToml {
        npx: Some(PackageDistribution {
            package: "@zed-industries/claude-agent-acp".to_string(),
            args: vec![],
        }),
        ..Default::default()
    };
    let resolved = dist.resolve().unwrap();
    assert!(matches!(resolved, ResolvedDistribution::Npx { .. }));
    if let ResolvedDistribution::Npx { package, args } = resolved {
        assert_eq!(package, "@zed-industries/claude-agent-acp");
        assert!(args.is_empty());
    }
}

#[test]
fn test_agent_distribution_resolve_local() {
    let dist = AgentDistributionToml {
        local: Some(LocalDistribution {
            command: "/usr/bin/agent".to_string(),
            args: vec!["--acp".to_string()],
            env: HashMap::from([("KEY".to_string(), "val".to_string())]),
        }),
        ..Default::default()
    };
    let resolved = dist.resolve().unwrap();
    assert!(matches!(resolved, ResolvedDistribution::Local { .. }));
    if let ResolvedDistribution::Local { command, args, env } = resolved {
        assert_eq!(command, "/usr/bin/agent");
        assert_eq!(args, vec!["--acp"]);
        assert_eq!(env.get("KEY").unwrap(), "val");
    }
}

#[test]
fn test_agent_distribution_resolve_uvx() {
    let dist = AgentDistributionToml {
        uvx: Some(PackageDistribution {
            package: "kimi-cli".to_string(),
            args: vec!["acp".to_string()],
        }),
        ..Default::default()
    };
    let resolved = dist.resolve().unwrap();
    assert!(matches!(resolved, ResolvedDistribution::Uvx { .. }));
}

#[test]
fn test_agent_distribution_resolve_pipx() {
    let dist = AgentDistributionToml {
        pipx: Some(PackageDistribution {
            package: "py-agent".to_string(),
            args: vec![],
        }),
        ..Default::default()
    };
    let resolved = dist.resolve().unwrap();
    assert!(matches!(resolved, ResolvedDistribution::Pipx { .. }));
}

#[test]
fn test_agent_distribution_resolve_bunx() {
    let dist = AgentDistributionToml {
        bunx: Some(PackageDistribution {
            package: "@google/gemini-cli".to_string(),
            args: vec!["--experimental-acp".to_string()],
        }),
        ..Default::default()
    };
    let resolved = dist.resolve().unwrap();
    if let ResolvedDistribution::Bunx { package, args } = resolved {
        assert_eq!(package, "@google/gemini-cli");
        assert_eq!(args, vec!["--experimental-acp"]);
    } else {
        panic!("Expected Bunx variant");
    }
}

// ========================================================================
// HistorySearch Hotkey Action Tests
// ========================================================================

#[test]
fn test_hotkey_config_from_toml_custom_history_search() {
    use pretty_assertions::assert_eq;
    let config: TuiConfigToml = toml::from_str(
        r#"
[hotkeys]
history_search = "ctrl+s"
"#,
    )
    .unwrap();
    let resolved = HotkeyConfig::from_toml(&config.hotkeys);
    assert_eq!(
        resolved.binding_for(HotkeyAction::HistorySearch),
        &HotkeyBinding::from_str("ctrl+s")
    );
}

#[test]
fn test_hotkey_config_set_binding_history_search() {
    use pretty_assertions::assert_eq;
    let mut config = HotkeyConfig::default();
    config.set_binding(
        HotkeyAction::HistorySearch,
        HotkeyBinding::from_str("alt+r"),
    );
    assert_eq!(
        config.binding_for(HotkeyAction::HistorySearch),
        &HotkeyBinding::from_str("alt+r")
    );
}
