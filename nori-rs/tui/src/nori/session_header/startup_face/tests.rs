use std::time::Duration;
use std::time::Instant;

use pretty_assertions::assert_eq;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::text::Line;
use unicode_width::UnicodeWidthStr;

use super::StartupFace;

const VERSION: &str = "0.42.0";

fn text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

fn title_at(face: &StartupFace, now: Instant) -> String {
    face.tick(now);
    text(&face.title_line(VERSION))
}

/// The distinct titles shown from `from` to `to`, each with how long it stayed
/// on screen, sampled every 10 ms. `ready` marks readiness at that offset.
fn timeline(
    face: &StartupFace,
    start: Instant,
    from_ms: i64,
    to_ms: i64,
    ready_ms: Option<i64>,
) -> Vec<(String, i64)> {
    assert!(
        ready_ms.is_none_or(|ms| ms % 10 == 0),
        "readiness is sampled every 10 ms"
    );
    let ms = |at: i64| Duration::from_millis(at.try_into().unwrap());
    let mut shown: Vec<(String, i64)> = Vec::new();
    let mut at = from_ms;
    while at < to_ms {
        let now = start + ms(at);
        if ready_ms == Some(at) {
            face.ready(now);
        }
        let title = title_at(face, now);
        match shown.last_mut() {
            Some((last, ms)) if *last == title => *ms += 10,
            _ => shown.push((title, 10)),
        }
        at += 10;
    }
    shown
}

fn closed_frames(shown: &[(String, i64)]) -> usize {
    shown.iter().filter(|(title, _)| title == "  - _ ▁").count()
}

fn finale(last_letter_ms: i64) -> Vec<(String, i64)> {
    let mut expected = vec![("  〉_ -".to_string(), 240), ("  〉_ ▐".to_string(), 200)];
    let word = "nori CLI";
    for end in 1..=word.len() {
        let ms = if end == word.len() {
            last_letter_ms
        } else {
            60
        };
        expected.push((format!("  〉{}_ ▐", &word[..end]), ms));
    }
    expected.push(("  〉nori CLI".to_string(), 120));
    expected
}

#[test]
fn starts_closed_then_opens_both_eyes_together() {
    let start = Instant::now();
    let face = StartupFace::new(start, 1);

    let shown = timeline(&face, start, 0, 600, None);

    assert_eq!(
        shown[..3].to_vec(),
        vec![
            ("  - _ ▁".to_string(), 400),
            ("  › _ ▗".to_string(), 80),
            ("  〉_ ▐".to_string(), 120),
        ]
    );
}

#[test]
fn blinks_the_chosen_number_of_times_at_80ms_per_frame_then_holds_open_quietly() {
    for blinks in 1..=3 {
        let start = Instant::now();
        let face = StartupFace::new(start, blinks);

        let shown = timeline(&face, start, 0, 30_000, None);

        // The opening frame plus one closed frame per blink.
        assert_eq!(closed_frames(&shown), 1 + usize::try_from(blinks).unwrap());
        let blink = shown
            .windows(3)
            .skip(3)
            .find(|w| w[1].0 == "  - _ ▁")
            .expect("a blink");
        assert_eq!(
            blink.to_vec(),
            vec![
                ("  › _ ▗".to_string(), 80),
                ("  - _ ▁".to_string(), 80),
                ("  › _ ▗".to_string(), 80),
            ]
        );
        assert_eq!(shown.last().map(|(t, _)| t.as_str()), Some("  〉_ ▐"));
        assert_eq!(face.tick(start + Duration::from_secs(30)), None);
        assert!(!face.is_finished());
    }
}

#[test]
fn readiness_while_holding_winks_then_retypes_into_the_versioned_wordmark() {
    let start = Instant::now();
    let face = StartupFace::new(start, 1);

    let shown = timeline(&face, start, 10_000, 13_000, Some(10_000));

    let mut expected = vec![("  〉_ ▐".to_string(), 120)];
    expected.extend(finale(300));
    expected.push(("  〉nori CLI v0.42.0".to_string(), 1_600));
    assert_eq!(shown, expected);
    assert!(face.is_finished());
    assert_eq!(face.tick(start + Duration::from_secs(13)), None);
}

#[test]
fn readiness_mid_blink_finishes_that_blink_and_drops_the_rest() {
    let start = Instant::now();
    let face = StartupFace::new(start, 3);
    let full = timeline(&StartupFace::new(start, 3), start, 0, 10_000, None);
    let first_blink_ms: i64 = full.iter().take(3).map(|(_, ms)| ms).sum();

    // Ready 40 ms into the first blink.
    let shown = timeline(&face, start, 0, 6_000, Some(first_blink_ms + 40));

    assert_eq!(closed_frames(&shown), 2);
    let wink = shown
        .iter()
        .position(|(t, _)| t == "  〉_ -")
        .expect("wink");
    assert_eq!(
        shown[wink - 2..wink].to_vec(),
        vec![("  › _ ▗".to_string(), 80), ("  〉_ ▐".to_string(), 120),]
    );
    assert_eq!(
        shown.last().map(|(t, _)| t.as_str()),
        Some("  〉nori CLI v0.42.0")
    );
}

#[test]
fn readiness_before_the_eyes_open_opens_them_first() {
    let start = Instant::now();
    let face = StartupFace::new(start, 2);

    let shown = timeline(&face, start, 0, 3_000, Some(50));

    assert_eq!(
        shown[..3].to_vec(),
        vec![
            ("  - _ ▁".to_string(), 50),
            ("  › _ ▗".to_string(), 80),
            ("  〉_ ▐".to_string(), 120),
        ]
    );
    assert_eq!(shown[3..3 + finale(300).len()].to_vec(), finale(300));
    assert_eq!(closed_frames(&shown), 1);
}

#[test]
fn a_second_readiness_changes_nothing() {
    let start = Instant::now();
    let once = StartupFace::new(start, 1);
    let twice = StartupFace::new(start, 1);

    let expected = timeline(&once, start, 3_000, 6_000, Some(3_000));
    let mut shown = timeline(&twice, start, 3_000, 3_500, Some(3_000));
    twice.ready(start + Duration::from_millis(3_500));
    shown.extend(timeline(&twice, start, 3_500, 6_000, None));
    let merged = shown
        .into_iter()
        .fold(Vec::<(String, i64)>::new(), |mut acc, (t, ms)| {
            match acc.last_mut() {
                Some((last, total)) if *last == t => *total += ms,
                _ => acc.push((t, ms)),
            }
            acc
        });

    assert_eq!(merged, expected);
}

#[test]
fn freezing_before_readiness_keeps_the_open_face_and_never_winks() {
    let start = Instant::now();
    let face = StartupFace::new(start, 2);

    face.freeze();
    let shown = timeline(&face, start, 1_500, 5_000, Some(2_000));

    assert_eq!(shown, vec![("  〉_ ▐".to_string(), 3_500)]);
    assert!(face.is_finished());
    assert_eq!(face.tick(start + Duration::from_secs(5)), None);
}

#[test]
fn freezing_during_the_finale_settles_on_the_wordmark() {
    let start = Instant::now();
    let face = StartupFace::new(start, 1);
    face.ready(start + Duration::from_secs(5));

    face.tick(start + Duration::from_millis(5_300));
    face.freeze();

    assert_eq!(
        title_at(&face, start + Duration::from_millis(5_310)),
        "  〉nori CLI v0.42.0"
    );
    assert!(face.is_finished());
}

#[test]
fn the_static_wordmark_needs_no_frames() {
    let start = Instant::now();
    let face = StartupFace::settled();

    assert_eq!(title_at(&face, start), "  〉nori CLI v0.42.0");
    assert_eq!(face.tick(start), None);
    assert!(face.is_finished());
}

#[test]
fn every_face_frame_is_five_cells_with_green_eyes_and_a_plain_mouth() {
    let start = Instant::now();
    let face = StartupFace::new(start, 1);
    face.ready(start + Duration::from_secs(3));
    let mut faces = Vec::new();
    let mut at = 0;
    // Up to the first typed letter: opening, one blink, the wink.
    while at < 3_560 {
        let now = start + Duration::from_millis(at);
        face.tick(now);
        let line = face.title_line(VERSION);
        let title = text(&line);
        if faces.last() != Some(&title) {
            assert_eq!(UnicodeWidthStr::width(title.as_str()), 2 + 5, "{title}");
            for span in &line.spans {
                let content = span.content.trim();
                if content == "_" {
                    assert_eq!(span.style.fg, None, "mouth in {title}");
                } else if !content.is_empty() {
                    assert_eq!(
                        span.style.fg,
                        Some(Color::Green),
                        "eye {content} in {title}"
                    );
                }
            }
            faces.push(title);
        }
        at += 10;
    }
    assert_eq!(
        faces,
        vec![
            "  - _ ▁",
            "  › _ ▗",
            "  〉_ ▐",
            "  › _ ▗",
            "  - _ ▁",
            "  › _ ▗",
            "  〉_ ▐",
            "  〉_ -",
            "  〉_ ▐"
        ]
    );
}

#[test]
fn the_wordmark_is_a_green_bracket_bold_name_and_dim_version() {
    let face = StartupFace::settled();
    let line = face.title_line(VERSION);

    let styled: Vec<(String, Option<Color>, bool, bool)> = line
        .spans
        .iter()
        .filter(|span| !span.content.trim().is_empty())
        .map(|span| {
            (
                span.content.trim().to_string(),
                span.style.fg,
                span.style.add_modifier.contains(Modifier::BOLD),
                span.style.add_modifier.contains(Modifier::DIM),
            )
        })
        .collect();
    assert_eq!(
        styled,
        vec![
            ("〉".to_string(), Some(Color::Green), false, false),
            ("nori CLI".to_string(), None, true, false),
            ("v0.42.0".to_string(), None, false, true),
        ]
    );
}

#[test]
fn frame_sheet_snapshot() {
    let start = Instant::now();
    let face = StartupFace::new(start, 1);
    face.ready(start + Duration::from_secs(3));
    let sheet = timeline(&face, start, 0, 6_000, None)
        .into_iter()
        .map(|(title, ms)| {
            let pad = 22 - UnicodeWidthStr::width(title.as_str());
            format!("{title}{:pad$}{ms:>5} ms", "")
        })
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(sheet);
}

#[test]
fn three_blinks_frame_sheet_snapshot() {
    let start = Instant::now();
    let face = StartupFace::new(start, 3);
    let sheet = timeline(&face, start, 0, 6_000, None)
        .into_iter()
        .map(|(title, ms)| format!("{title}  {ms} ms"))
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(sheet);
}

#[test]
fn readiness_mid_blink_still_wakes_for_every_remaining_blink_frame() {
    let start = Instant::now();
    let face = StartupFace::new(start, 1);
    // The only blink starts at 1380 ms; readiness lands 10 ms into it.
    let ready = start + Duration::from_millis(1_390);
    face.ready(ready);

    let mut now = ready;
    let mut shown = vec![title_at(&face, now)];
    // Follow only the wakeups the face asks for, as the app does.
    while let Some(next) = face.tick(now) {
        now += next;
        let title = title_at(&face, now);
        if shown.last() != Some(&title) {
            shown.push(title);
        }
        if shown.last().map(String::as_str) == Some("  〉_ -") {
            break;
        }
    }

    assert_eq!(
        shown,
        vec!["  › _ ▗", "  - _ ▁", "  › _ ▗", "  〉_ ▐", "  〉_ -"]
    );
}
