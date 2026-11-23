//! Model preferences are scoped to the agent that owns a registered name.
use nori_acp_host::registry::initialize_registry;
use nori_acp_host::registry::resolve_agent_selection;
use pretty_assertions::assert_eq;
use std::collections::HashMap;

#[test]
fn custom_aliases_keep_their_own_models_and_canonical_keys_win() {
    let custom = serde_json::from_value(serde_json::json!({
        "name": "Example", "slug": "grok",
        "distribution": {"local": {"command": "example-agent"}}
    }))
    .unwrap();
    initialize_registry(vec![custom]).unwrap();

    let mut models = HashMap::from([("grok".to_owned(), "custom-choice".to_owned())]);
    assert_eq!(
        resolve_agent_selection("grok", &models),
        ("grok".into(), Some("custom-choice".into()))
    );
    assert_eq!(
        resolve_agent_selection("grok-build", &models),
        ("grok-build".into(), None)
    );

    models.insert("grok-build".into(), "xai-choice".into());
    assert_eq!(
        resolve_agent_selection("grok-build", &models),
        ("grok-build".into(), Some("xai-choice".into()))
    );

    models.insert("antigravity".into(), "legacy-choice".into());
    assert_eq!(
        resolve_agent_selection("gemini", &models),
        ("gemini".into(), Some("legacy-choice".into()))
    );
    models.insert("gemini".into(), "new-choice".into());
    assert_eq!(
        resolve_agent_selection("antigravity", &models),
        ("gemini".into(), Some("new-choice".into()))
    );
    assert_eq!(
        resolve_agent_selection("grok", &models),
        ("grok".into(), Some("custom-choice".into()))
    );
}
