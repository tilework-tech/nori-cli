use pretty_assertions::assert_eq;

use super::output_cap::CappedLines;
use super::output_cap::cap_lines;

fn numbered(count: i32) -> Vec<String> {
    (1..=count).map(|n| format!("row {n}")).collect()
}

/// Renders a capped list as plain rows, writing the marker as `[N hidden]`.
fn shown(lines: &[String], max_lines: Option<i32>) -> Vec<String> {
    match cap_lines(lines, max_lines) {
        CappedLines::All(all) => all.to_vec(),
        CappedLines::Split { head, hidden, tail } => head
            .iter()
            .cloned()
            .chain(std::iter::once(format!("[{hidden} hidden]")))
            .chain(tail.iter().cloned())
            .collect(),
    }
}

#[test]
fn no_limit_shows_every_line() {
    assert_eq!(shown(&numbered(50), None), numbered(50));
}

#[test]
fn empty_output_stays_empty_under_any_limit() {
    for limit in [1, 2, 5] {
        assert_eq!(shown(&[], Some(limit)), Vec::<String>::new(), "{limit}");
    }
}

#[test]
fn output_within_the_limit_is_untouched() {
    assert_eq!(shown(&numbered(3), Some(5)), numbered(3));
    assert_eq!(shown(&numbered(5), Some(5)), numbered(5));
}

#[test]
fn marker_counts_toward_the_limit_and_the_odd_line_goes_to_the_tail() {
    assert_eq!(
        shown(&numbered(10), Some(6)),
        vec!["row 1", "row 2", "[5 hidden]", "row 8", "row 9", "row 10"]
    );
    assert_eq!(
        shown(&numbered(10), Some(5)),
        vec!["row 1", "row 2", "[6 hidden]", "row 9", "row 10"]
    );
}

#[test]
fn tiny_limits_keep_the_marker_and_favor_the_tail() {
    assert_eq!(shown(&numbered(4), Some(1)), vec!["[4 hidden]"]);
    assert_eq!(shown(&numbered(4), Some(2)), vec!["[3 hidden]", "row 4"]);
    assert_eq!(
        shown(&numbered(4), Some(3)),
        vec!["row 1", "[2 hidden]", "row 4"]
    );
}

#[test]
fn one_line_over_the_limit_hides_two_lines_so_the_marker_never_replaces_one() {
    assert_eq!(
        shown(&numbered(6), Some(5)),
        vec!["row 1", "row 2", "[2 hidden]", "row 5", "row 6"]
    );
}

#[test]
fn non_positive_limits_show_every_line() {
    assert_eq!(shown(&numbered(8), Some(0)), numbered(8));
    assert_eq!(shown(&numbered(8), Some(-2)), numbered(8));
}
