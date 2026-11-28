//! Reports reach the terminal as OSC 7501 sequences, only when the status
//! changes, and are cleared on the way out.
use super::*;
use crate::program_status::ProgramState;
use base64::Engine;
use pretty_assertions::assert_eq;

fn config(program_status: bool, approval_titles: bool) -> nori_config::NoriConfig {
    nori_config::NoriConfig {
        program_status,
        program_status_approval_titles: approval_titles,
        ..Default::default()
    }
}

fn reporter(approval_titles: bool) -> ProgramStatusReporter {
    ProgramStatusReporter::from_config(&config(true, approval_titles))
        .expect("program status is on")
}

fn status(state: ProgramState) -> ProgramStatus {
    ProgramStatus {
        state,
        approval_title: None,
    }
}

fn blocked(title: &str) -> ProgramStatus {
    ProgramStatus {
        state: ProgramState::Blocked,
        approval_title: Some(title.to_string()),
    }
}

/// Bytes written for each status in turn, one entry per call.
fn written(reporter: &mut ProgramStatusReporter, statuses: &[ProgramStatus]) -> Vec<String> {
    statuses
        .iter()
        .map(|status| {
            let mut out = Vec::new();
            reporter.report(&mut out, status).unwrap();
            String::from_utf8(out).unwrap()
        })
        .collect()
}

fn decoded_message(report: &str) -> String {
    let encoded = report
        .trim_end_matches("\u{1b}\\")
        .split(':')
        .find_map(|pair| pair.strip_prefix("msg="))
        .expect("report carries a message");
    String::from_utf8(
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn program_status_stays_silent_when_turned_off() {
    assert!(ProgramStatusReporter::from_config(&config(false, false)).is_none());
    assert!(ProgramStatusReporter::from_config(&config(false, true)).is_none());
}

#[test]
fn a_default_config_reports_with_the_generic_approval_message() {
    let mut reporter = ProgramStatusReporter::from_config(&nori_config::NoriConfig::default())
        .expect("program status is on by default");
    let mut out = Vec::new();
    reporter
        .report(&mut out, &blocked("Run cargo test"))
        .unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "\x1b]7501;state=blocked:app=nori:kind=permission:msg=QXBwcm92YWwgbmVlZGVk\x1b\\"
    );
}

#[test]
fn each_state_is_reported_as_an_osc_7501_sequence() {
    let mut reporter = reporter(false);
    assert_eq!(
        written(
            &mut reporter,
            &[
                status(ProgramState::Idle),
                status(ProgramState::Working),
                blocked("Run cargo test"),
                status(ProgramState::Done),
                status(ProgramState::Error),
            ]
        ),
        vec![
            "\u{1b}]7501;state=idle:app=nori\u{1b}\\",
            "\u{1b}]7501;state=working:app=nori\u{1b}\\",
            "\u{1b}]7501;state=blocked:app=nori:kind=permission:msg=QXBwcm92YWwgbmVlZGVk\u{1b}\\",
            "\u{1b}]7501;state=done:app=nori\u{1b}\\",
            "\u{1b}]7501;state=error:app=nori\u{1b}\\",
        ]
    );
}

#[test]
fn an_unchanged_status_is_not_written_again() {
    let mut reporter = reporter(false);
    assert_eq!(
        written(
            &mut reporter,
            &[
                status(ProgramState::Working),
                status(ProgramState::Working),
                status(ProgramState::Done),
                status(ProgramState::Done),
            ]
        ),
        vec![
            "\u{1b}]7501;state=working:app=nori\u{1b}\\",
            "",
            "\u{1b}]7501;state=done:app=nori\u{1b}\\",
            "",
        ]
    );
}

#[test]
fn a_status_seen_before_is_reported_again_after_a_change() {
    let mut reporter = reporter(false);
    let reports = written(
        &mut reporter,
        &[
            status(ProgramState::Done),
            status(ProgramState::Working),
            status(ProgramState::Done),
        ],
    );
    assert_eq!(reports[2], reports[0]);
}

#[test]
fn a_new_approval_title_is_not_a_change_while_titles_stay_private() {
    let mut reporter = reporter(false);
    let reports = written(&mut reporter, &[blocked("first"), blocked("second")]);
    assert_eq!(reports[1], "");
}

#[test]
fn opted_in_reports_carry_the_approval_title() {
    let mut reporter = reporter(true);
    let reports = written(
        &mut reporter,
        &[
            blocked("Run cargo test"),
            blocked("rm -rf build && echo done"),
        ],
    );
    assert_eq!(
        reports,
        vec![
            "\u{1b}]7501;state=blocked:app=nori:kind=permission:msg=UnVuIGNhcmdvIHRlc3Q=\u{1b}\\",
            "\u{1b}]7501;state=blocked:app=nori:kind=permission:msg=cm0gLXJmIGJ1aWxkICYmIGVjaG8gZG9uZQ==\u{1b}\\",
        ]
    );
}

#[test]
fn opted_in_reports_fall_back_to_the_generic_message_without_a_title() {
    let mut reporter = reporter(true);
    let reports = written(
        &mut reporter,
        &[ProgramStatus {
            state: ProgramState::Blocked,
            approval_title: None,
        }],
    );
    assert_eq!(decoded_message(&reports[0]), "Approval needed");
}

#[test]
fn approval_titles_lose_control_and_direction_characters() {
    let mut reporter = reporter(true);
    let reports = written(
        &mut reporter,
        &[blocked(
            "run\ttests\nthen \u{1b}]0;spoof\u{7} \u{202e}deploy",
        )],
    );
    assert_eq!(
        decoded_message(&reports[0]),
        "run tests then  ]0;spoof   deploy"
    );
}

#[test]
fn long_approval_titles_are_cut_to_the_protocol_limit_on_a_character_boundary() {
    let mut reporter = reporter(true);
    let title = "é".repeat(1500);
    let reports = written(&mut reporter, &[blocked(&title)]);
    let message = decoded_message(&reports[0]);
    assert_eq!(message, "é".repeat(1024));
}

#[test]
fn clearing_removes_a_reported_status_once() {
    let mut reporter = reporter(false);
    written(&mut reporter, &[status(ProgramState::Done)]);

    let mut first = Vec::new();
    reporter.clear(&mut first).unwrap();
    let mut second = Vec::new();
    reporter.clear(&mut second).unwrap();

    assert_eq!(
        (
            String::from_utf8(first).unwrap(),
            String::from_utf8(second).unwrap()
        ),
        ("\u{1b}]7501;state=clear\u{1b}\\".to_string(), String::new())
    );
}

#[test]
fn clearing_before_any_report_writes_nothing() {
    let mut reporter = reporter(false);
    let mut out = Vec::new();
    reporter.clear(&mut out).unwrap();
    assert!(out.is_empty());
}

#[test]
fn a_status_after_clearing_is_reported_again() {
    let mut reporter = reporter(false);
    written(&mut reporter, &[status(ProgramState::Done)]);
    reporter.clear(&mut Vec::new()).unwrap();
    assert_eq!(
        written(&mut reporter, &[status(ProgramState::Done)]),
        vec!["\u{1b}]7501;state=done:app=nori\u{1b}\\"]
    );
}
