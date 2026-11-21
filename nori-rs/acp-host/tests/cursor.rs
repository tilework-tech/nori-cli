//! Cursor's selected model is delivered to the native CLI.
use nori_acp_host::registry::get_agent_config;
use pretty_assertions::assert_eq;

#[test]
fn cursor_alias_launches_acp_with_the_requested_model() {
    let mut config = get_agent_config("cursor-agent").expect("built-in Cursor agent");
    config.inject_model("example-model").unwrap();
    assert_eq!(config.provider_slug, "cursor");
    assert_eq!(config.args, ["acp", "--model", "example-model"]);
}
