use super::*;
use crate::nori::skillset_picker::SkillsetProject;
use nori_skillsets::SkillsetList;

fn drain(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> Vec<AppEvent> {
    std::iter::from_fn(|| rx.try_recv().ok()).collect()
}

fn project() -> SkillsetProject {
    SkillsetProject::Home(PathBuf::from("/home/user"))
}

#[tokio::test]
async fn failed_listing_releases_a_deferred_session_start() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.on_skillset_list_result(project(), Err("not the skillsets v2 CLI".to_string()));

    let events = drain(&mut rx);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AppEvent::SkillsetPickerDismissed)),
        "a failed listing must dismiss the picker flow, got {events:?}"
    );
}

#[tokio::test]
async fn empty_listing_releases_a_deferred_session_start() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.on_skillset_list_result(
        project(),
        Ok(SkillsetList {
            project: PathBuf::from("/home/user"),
            repository: None,
            skillsets: vec![],
            warning: None,
        }),
    );

    let events = drain(&mut rx);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AppEvent::SkillsetPickerDismissed)),
        "an empty listing must dismiss the picker flow, got {events:?}"
    );
}

#[tokio::test]
async fn layer_result_reports_the_stack_and_every_note() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.on_skillset_change_result(
        "team",
        true,
        Ok(nori_skillsets::LayerChange {
            layers: vec!["base".to_string(), "team".to_string()],
            notes: vec!["refused AGENTS.md: edited since last compile".to_string()],
        }),
    );

    let text = lines_to_single_string(&drain_insert_history(&mut rx).concat());
    assert!(
        text.contains("Layered skillset \"team\" (layers: base, team)"),
        "{text}"
    );
    assert!(
        text.contains("refused AGENTS.md: edited since last compile"),
        "{text}"
    );
}

#[tokio::test]
async fn failed_unlayer_reports_the_cli_error() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.on_skillset_change_result("team", false, Err("manifest is corrupt".to_string()));

    let text = lines_to_single_string(&drain_insert_history(&mut rx).concat());
    assert!(
        text.contains("Failed to unlayer skillset 'team': manifest is corrupt"),
        "{text}"
    );
}

#[tokio::test]
async fn unreadable_releases_are_reported_alongside_layered_skillsets() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.on_skillset_list_result(
        project(),
        Ok(SkillsetList {
            project: PathBuf::from("/home/user"),
            repository: Some("https://registrar.example/skillsets.git".to_string()),
            skillsets: vec![nori_skillsets::SkillsetEntry {
                name: "team".to_string(),
                layered: true,
                version: Some("1.0.0".to_string()),
            }],
            warning: Some("authentication failed".to_string()),
        }),
    );

    let text = lines_to_single_string(&drain_insert_history(&mut rx).concat());
    assert!(
        text.contains("published releases could not be read")
            && text.contains("authentication failed"),
        "{text}"
    );
}
