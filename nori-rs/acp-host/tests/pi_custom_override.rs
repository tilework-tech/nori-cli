//! A custom agent that takes over the `pi` slug is not offered Pi's token
//! providers.
use nori_acp_host::registry::get_agent_launch_config;
use nori_acp_host::registry::initialize_registry;
use nori_config::token_providers::Preset;
use nori_config::token_providers::Protocol;
use nori_config::token_providers::TokenProvider;

#[test]
fn a_custom_pi_agent_is_not_offered_token_providers() {
    let binding = TokenProvider {
        name: "lab".into(),
        preset: Preset::Custom,
        protocol: Protocol::OpenaiCompletions,
        base_url: "http://127.0.0.1:4567/v1".into(),
        env_key: "EXAMPLE_KEY".into(),
        model: "lab-small".into(),
    };
    assert!(get_agent_launch_config("pi", Some(&binding)).is_ok());
    let custom = serde_json::from_value(serde_json::json!({
        "name": "Local Pi", "slug": "pi",
        "distribution": {"local": {"command": "local-pi-acp"}}
    }))
    .unwrap();
    initialize_registry(vec![custom]).unwrap();
    assert!(get_agent_launch_config("pi", Some(&binding)).is_err());
}
