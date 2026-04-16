mod strip_ansi_codes_tests {
    use super::super::strip_ansi_codes;

    #[test]
    fn strips_csi_color_codes() {
        // CSI sequence: ESC [ followed by params and ending with letter
        assert_eq!(strip_ansi_codes("\x1b[31mred text\x1b[0m"), "red text");
    }

    #[test]
    fn strips_multiple_csi_sequences() {
        assert_eq!(
            strip_ansi_codes("\x1b[1m\x1b[32mbold green\x1b[0m normal"),
            "bold green normal"
        );
    }

    #[test]
    fn strips_osc_sequence_with_bel() {
        // OSC sequence: ESC ] ... BEL
        assert_eq!(
            strip_ansi_codes("\x1b]0;window title\x07some text"),
            "some text"
        );
    }

    #[test]
    fn strips_osc_sequence_with_st() {
        // OSC sequence: ESC ] ... ESC \
        assert_eq!(
            strip_ansi_codes("\x1b]0;window title\x1b\\some text"),
            "some text"
        );
    }

    #[test]
    fn strips_carriage_return() {
        // Windows-style line endings should become Unix-style
        assert_eq!(strip_ansi_codes("line1\r\nline2"), "line1\nline2");
    }

    #[test]
    fn preserves_plain_text() {
        assert_eq!(strip_ansi_codes("plain text"), "plain text");
    }

    #[test]
    fn handles_empty_string() {
        assert_eq!(strip_ansi_codes(""), "");
    }

    #[test]
    fn handles_text_with_only_ansi() {
        assert_eq!(strip_ansi_codes("\x1b[31m\x1b[0m"), "");
    }

    #[test]
    fn strips_cursor_movement_codes() {
        // CSI sequences for cursor movement
        assert_eq!(strip_ansi_codes("\x1b[2Jtext\x1b[H"), "text");
    }
}
