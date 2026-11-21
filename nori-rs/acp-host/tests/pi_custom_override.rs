//! A custom agent that takes over the `pi` slug is not offered Pi's token
//! providers.
use nori_acp_host::registry::initialize_registry;
use nori_acp_host::token_provider::compatible_presets;

#[test]
fn a_custom_pi_agent_is_not_offered_token_providers() {
    assert!(!compatible_presets("pi").is_empty());
    let custom = serde_json::from_value(serde_json::json!({
        "name": "Local Pi", "slug": "pi",
        "distribution": {"local": {"command": "local-pi-acp"}}
    }))
    .unwrap();
    initialize_registry(vec![custom]).unwrap();
    assert!(compatible_presets("pi").is_empty());
}
