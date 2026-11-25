//! `/new DIR` and `@` matches in the plain terminal: the folder resolves
//! against the session on screen, and a folder that is not there is refused
//! with the text kept.

use super::*;
use pretty_assertions::assert_eq;

fn submit(chat: &mut ChatWidget, text: &str) {
    chat.set_composer_text(text.to_owned());
    chat.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
}

fn new_session_folders(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> Vec<PathBuf> {
    std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|event| match event {
            AppEvent::NewSessionIn(folder) => Some(folder),
            _ => None,
        })
        .collect()
}

#[test]
fn new_with_a_folder_starts_there_resolved_from_the_sessions_folder() {
    let session = tempfile::tempdir().unwrap();
    std::fs::create_dir(session.path().join("sub")).unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.config.cwd = session.path().to_path_buf();

    submit(&mut chat, "/new sub");
    submit(&mut chat, "/new ./sub/");
    submit(&mut chat, &format!("/new {}", elsewhere.path().display()));
    assert_eq!(
        new_session_folders(&mut rx),
        vec![
            session.path().join("sub"),
            session.path().join("sub"),
            elsewhere.path().to_path_buf(),
        ]
    );
    assert_eq!(chat.composer_text(), "");
}

#[test]
fn new_with_a_missing_folder_is_refused_and_keeps_the_text() {
    let session = tempfile::tempdir().unwrap();
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.config.cwd = session.path().to_path_buf();

    submit(&mut chat, "/new missing");
    let events: Vec<AppEvent> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    let shown: Vec<String> = events
        .iter()
        .filter_map(|event| match event {
            AppEvent::InsertHistoryCell(cell) => {
                Some(lines_to_single_string(&cell.display_lines(200)))
            }
            _ => None,
        })
        .collect();
    assert!(
        events.iter().all(|event| !matches!(
            event,
            AppEvent::NewSessionIn(_) | AppEvent::NewSession | AppEvent::HostRequest { .. }
        )),
        "{events:?}"
    );
    let missing = session.path().join("missing").display().to_string();
    assert!(
        shown
            .iter()
            .any(|text| text.contains(&format!("No folder at {missing}"))),
        "{shown:?}"
    );
    assert_eq!(chat.composer_text(), "/new missing");
}

#[test]
fn host_matches_reach_the_mention_popup() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    type_text(&mut chat, "@lex");
    chat.apply_file_search_result(
        "lex".into(),
        crate::host_client::file_matches(serde_json::json!({
            "root": "/srv/parser",
            "matches": [{"path": "src/lexer.rs", "indices": [4, 5, 6]}],
        }))
        .unwrap(),
    );
    use crate::render::renderable::Renderable;
    let area = ratatui::layout::Rect::new(0, 0, 80, chat.bottom_pane.desired_height(80));
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    chat.bottom_pane.render(area, &mut buffer);
    let rendered: String = (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                + "\n"
        })
        .collect();
    assert!(rendered.contains("src/lexer.rs"), "{rendered}");
}

fn press_tab(chat: &mut ChatWidget, text: &str) {
    chat.set_composer_text(text.to_owned());
    chat.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
}

fn shown(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> Vec<String> {
    std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|event| match event {
            AppEvent::InsertHistoryCell(cell) => {
                Some(lines_to_single_string(&cell.display_lines(200)))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn tab_completes_the_new_folder_from_the_sessions_folder() {
    let session = tempfile::tempdir().unwrap();
    for folder in ["orchard", "greenhouse", "greenery", ".hidden"] {
        std::fs::create_dir(session.path().join(folder)).unwrap();
    }
    std::fs::create_dir(session.path().join("orchard/pears")).unwrap();
    std::fs::write(session.path().join("greens.txt"), "").unwrap();
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.config.cwd = session.path().to_path_buf();

    press_tab(&mut chat, "/new or");
    assert_eq!(chat.composer_text(), "/new orchard/");
    chat.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(chat.composer_text(), "/new orchard/pears/");
    assert!(shown(&mut rx).is_empty());

    press_tab(&mut chat, "/new gr");
    assert_eq!(chat.composer_text(), "/new green");
    let listed = shown(&mut rx).join("\n");
    assert!(
        listed.contains("greenery/") && listed.contains("greenhouse/"),
        "{listed}"
    );
    assert!(
        !listed.contains(".hidden") && !listed.contains("greens.txt"),
        "{listed}"
    );

    press_tab(&mut chat, "/new .h");
    assert_eq!(chat.composer_text(), "/new .hidden/");
    press_tab(&mut chat, "/new pl");
    assert_eq!(chat.composer_text(), "/new pl");
    assert!(shown(&mut rx).is_empty());
}

#[test]
fn tab_leaves_other_text_alone() {
    let session = tempfile::tempdir().unwrap();
    std::fs::create_dir(session.path().join("orchard")).unwrap();
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.config.cwd = session.path().to_path_buf();

    press_tab(&mut chat, "plant or");
    assert_eq!(chat.composer_text(), "plant or");
    press_tab(&mut chat, "/new or custom-agent");
    assert_eq!(chat.composer_text(), "/new or custom-agent");
    assert!(shown(&mut rx).is_empty());
}
