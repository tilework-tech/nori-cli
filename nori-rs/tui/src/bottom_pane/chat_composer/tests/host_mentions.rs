//! A file match in a terminal joined to a host names a file on the host's
//! disk, so picking an image match inserts its path for the agent to read
//! there instead of attaching a file of that name from this machine.

use super::*;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use ratatui::text::Line;

fn composer(joined: bool) -> ChatComposer {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let mut composer = ChatComposer::new(
        true,
        AppEventSender::new(tx),
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );
    if joined {
        composer.set_host_status(Some(Line::from("controlling")));
    }
    composer
}

fn pick_match(composer: &mut ChatComposer, path: &str) {
    type_chars_humanlike(composer, &['@', 's', 'h', 'o', 't']);
    composer.on_file_search_result(
        "shot".to_string(),
        vec![codex_file_search::FileMatch {
            score: 0,
            path: path.to_string(),
            indices: None,
        }],
    );
    composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
}

#[test]
fn an_image_match_from_the_host_is_inserted_as_its_path() {
    let dir = tempdir().unwrap();
    let image = dir.path().join("shot.png");
    ImageBuffer::<Rgba<u8>, Vec<u8>>::new(4, 3)
        .save(&image)
        .unwrap();
    let path = image.display().to_string();

    let mut joined = composer(true);
    pick_match(&mut joined, &path);
    assert!(
        joined.attached_images.is_empty(),
        "nothing read from this machine"
    );
    assert_eq!(joined.current_text(), format!("{path} "));

    let mut plain = composer(false);
    pick_match(&mut plain, &path);
    assert_eq!(
        plain.attached_images.len(),
        1,
        "plain nori attaches its own file"
    );
}
