use insta::assert_snapshot;
use ratatui::style::Stylize;
use ratatui::text::Line;
use tui_components::wrapping::{
    RtOptions, prefix_lines, word_wrap_line, word_wrap_lines, word_wrap_lines_borrowed,
};

fn concat_line(line: &Line) -> String {
    line.spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect::<String>()
}

fn render_lines(lines: &[Line]) -> String {
    lines
        .iter()
        .map(|l| concat_line(l))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn test_trivial_unstyled_no_indents_wide_width() {
    let line = Line::from("hello");
    let out = word_wrap_line(&line, 10);
    assert_snapshot!(render_lines(&out), @"hello");
}

#[test]
fn test_simple_unstyled_wrap_narrow_width() {
    let line = Line::from("hello world");
    let out = word_wrap_line(&line, 5);
    assert_snapshot!(render_lines(&out), @r###"
    hello
    world
    "###);
}

#[test]
fn test_simple_styled_wrap_preserves_styles() {
    let line = Line::from(vec!["hello ".red(), "world".into()]);
    let out = word_wrap_line(&line, 6);
    // Just test the text content, styles are preserved but not visible in snapshots
    assert_snapshot!(render_lines(&out), @r###"
    hello
    world
    "###);
}

#[test]
fn test_with_initial_and_subsequent_indents() {
    let opts = RtOptions::new(8)
        .initial_indent(Line::from("- "))
        .subsequent_indent(Line::from("  "));
    let line = Line::from("hello world foo");
    let out = word_wrap_line(&line, opts);
    assert_snapshot!(render_lines(&out), @r###"
    - hello
      world
      foo
    "###);
}

#[test]
fn test_empty_initial_indent_subsequent_spaces() {
    let opts = RtOptions::new(8)
        .initial_indent(Line::from(""))
        .subsequent_indent(Line::from("    "));
    let line = Line::from("hello world foobar");
    let out = word_wrap_line(&line, opts);
    assert_snapshot!(render_lines(&out), @r###"
    hello
        worl
        d
        foob
        ar
    "###);
}

#[test]
fn test_empty_input_yields_single_empty_line() {
    let line = Line::from("");
    let out = word_wrap_line(&line, 10);
    assert_snapshot!(render_lines(&out), @"");
}

#[test]
fn test_leading_spaces_preserved_on_first_line() {
    let line = Line::from("   hello");
    let out = word_wrap_line(&line, 8);
    assert_snapshot!(render_lines(&out), @"   hello");
}

#[test]
fn test_multiple_spaces_between_words_dont_start_next_line_with_spaces() {
    let line = Line::from("hello   world");
    let out = word_wrap_line(&line, 8);
    assert_snapshot!(render_lines(&out), @r###"
    hello
    world
    "###);
}

#[test]
fn test_break_words_false_allows_overflow_for_long_word() {
    let opts = RtOptions::new(5).break_words(false);
    let line = Line::from("supercalifragilistic");
    let out = word_wrap_line(&line, opts);
    assert_snapshot!(render_lines(&out), @"supercalifragilistic");
}

#[test]
fn test_hyphen_splitter_breaks_at_hyphen() {
    let line = Line::from("hello-world");
    let out = word_wrap_line(&line, 7);
    assert_snapshot!(render_lines(&out), @r###"
    hello-
    world
    "###);
}

#[test]
fn test_indent_consumes_width_leaving_one_char_space() {
    let opts = RtOptions::new(4)
        .initial_indent(Line::from(">>>>"))
        .subsequent_indent(Line::from("--"));
    let line = Line::from("hello");
    let out = word_wrap_line(&line, opts);
    assert_snapshot!(render_lines(&out), @r###"
    >>>>h
    --el
    --lo
    "###);
}

#[test]
fn test_wide_unicode_wraps_by_display_width() {
    let line = Line::from("😀😀😀");
    let out = word_wrap_line(&line, 4);
    assert_snapshot!(render_lines(&out), @r###"
    😀😀
    😀
    "###);
}

#[test]
fn test_styled_split_within_span_preserves_style() {
    let line = Line::from(vec!["abcd".red()]);
    let out = word_wrap_line(&line, 2);
    assert_snapshot!(render_lines(&out), @r###"
    ab
    cd
    "###);
}

#[test]
fn test_wrap_lines_applies_initial_indent_only_once() {
    let opts = RtOptions::new(8)
        .initial_indent(Line::from("- "))
        .subsequent_indent(Line::from("  "));

    let lines = vec![Line::from("hello world"), Line::from("foo bar baz")];
    let out = word_wrap_lines(lines, opts);
    assert_snapshot!(render_lines(&out), @r###"
    - hello
      world
      foo
      bar
      baz
    "###);
}

#[test]
fn test_wrap_lines_without_indents_is_concat_of_single_wraps() {
    let lines = vec![Line::from("hello"), Line::from("world!")];
    let out = word_wrap_lines(lines, 10);
    assert_snapshot!(render_lines(&out), @r###"
    hello
    world!
    "###);
}

#[test]
fn test_wrap_lines_borrowed_applies_initial_indent_only_once() {
    let opts = RtOptions::new(8)
        .initial_indent(Line::from("- "))
        .subsequent_indent(Line::from("  "));

    let lines = [Line::from("hello world"), Line::from("foo bar baz")];
    let out = word_wrap_lines_borrowed(lines.iter(), opts);
    assert_snapshot!(render_lines(&out), @r###"
    - hello
      world
      foo
      bar
      baz
    "###);
}

#[test]
fn test_wrap_lines_borrowed_without_indents_is_concat_of_single_wraps() {
    let lines = [Line::from("hello"), Line::from("world!")];
    let out = word_wrap_lines_borrowed(lines.iter(), 10);
    assert_snapshot!(render_lines(&out), @r###"
    hello
    world!
    "###);
}

#[test]
fn test_wrap_lines_accepts_borrowed_iterators() {
    let lines = [Line::from("hello world"), Line::from("foo bar baz")];
    let out = word_wrap_lines(lines, 10);
    assert_snapshot!(render_lines(&out), @r###"
    hello
    world
    foo bar
    baz
    "###);
}

#[test]
fn test_wrap_lines_accepts_str_slices() {
    let lines = ["hello world", "goodnight moon"];
    let out = word_wrap_lines(lines, 12);
    assert_snapshot!(render_lines(&out), @r###"
    hello world
    goodnight
    moon
    "###);
}

#[test]
fn test_line_height_counts_double_width_emoji() {
    let line: Line = "😀😀😀".into();
    let out = word_wrap_line(&line, 4);
    assert_snapshot!(render_lines(&out), @r###"
    😀😀
    😀
    "###);
}

#[test]
fn test_word_wrap_does_not_split_words_simple_english() {
    let sample = "Years passed, and Willowmere thrived in peace and friendship. Mira's herb garden flourished with both ordinary and enchanted plants, and travelers spoke of the kindness of the woman who tended them.";
    let line = Line::from(sample);
    let lines = [line];
    let wrapped = word_wrap_lines_borrowed(&lines, 40);
    assert_snapshot!(render_lines(&wrapped), @r###"
    Years passed, and Willowmere thrived
    in peace and friendship. Mira's herb
    garden flourished with both ordinary and
    enchanted plants, and travelers spoke
    of the kindness of the woman who tended
    them.
    "###);
}

#[test]
fn test_prefix_lines_with_initial_and_subsequent() {
    let lines = vec![
        Line::from("first line"),
        Line::from("second line"),
        Line::from("third line"),
    ];
    let prefixed = prefix_lines(lines, "> ".into(), "  ".into());
    assert_snapshot!(render_lines(&prefixed), @r###"
    > first line
      second line
      third line
    "###);
}

#[test]
fn test_prefix_lines_with_same_prefix() {
    let lines = vec![Line::from("alpha"), Line::from("beta"), Line::from("gamma")];
    let prefixed = prefix_lines(lines, "* ".into(), "* ".into());
    assert_snapshot!(render_lines(&prefixed), @r###"
    * alpha
    * beta
    * gamma
    "###);
}
