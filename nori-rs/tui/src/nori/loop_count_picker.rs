//! Loop count picker: custom `BottomPaneView` that shows preset loop counts
//! plus a "Custom..." option that allows typing an arbitrary value.

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Block;
use ratatui::widgets::Widget;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::BottomPaneView;
use crate::bottom_pane::CancellationEvent;
use crate::render::Insets;
use crate::render::RectExt as _;
use crate::render::renderable::Renderable;
use crate::style::user_message_style;

/// Maximum allowed custom loop count.
const MAX_LOOP_COUNT: i32 = 1000;

/// Preset options for the loop count picker (excluding "Custom...").
const PRESETS: [Option<i32>; 5] = [None, Some(2), Some(3), Some(5), Some(10)];

/// State for the loop count picker view.
pub(crate) struct LoopCountPickerView {
    /// Index into the combined list: presets + "Custom..." sentinel.
    selected_idx: usize,
    /// Whether we are in custom input mode.
    input_mode: bool,
    /// Buffer for the custom number being typed.
    input_buffer: String,
    /// Whether the view should be dismissed.
    complete: bool,
    /// Channel to send config change events.
    app_event_tx: AppEventSender,
    /// The currently configured loop count (for highlighting).
    current: Option<i32>,
}

impl LoopCountPickerView {
    /// Total number of items: presets + "Custom..." entry.
    fn item_count(&self) -> usize {
        PRESETS.len() + 1
    }

    /// Index of the "Custom..." entry.
    fn custom_idx(&self) -> usize {
        PRESETS.len()
    }

    /// Whether the current value matches one of the presets.
    fn current_is_preset(&self) -> bool {
        PRESETS.contains(&self.current)
    }

    pub fn new(current: Option<i32>, app_event_tx: AppEventSender) -> Self {
        // Determine initial selection: if current matches a preset, highlight it;
        // if it's a custom value, highlight the "Custom..." entry.
        let selected_idx = if let Some(idx) = PRESETS.iter().position(|&p| p == current) {
            idx
        } else {
            // Non-preset value: highlight "Custom..."
            PRESETS.len()
        };

        Self {
            selected_idx,
            input_mode: false,
            input_buffer: String::new(),
            complete: false,
            app_event_tx,
            current,
        }
    }

    fn move_up(&mut self) {
        if self.selected_idx == 0 {
            self.selected_idx = self.item_count() - 1;
        } else {
            self.selected_idx -= 1;
        }
    }

    fn move_down(&mut self) {
        self.selected_idx = (self.selected_idx + 1) % self.item_count();
    }

    fn select_current(&mut self) {
        if self.selected_idx == self.custom_idx() {
            // Enter input mode
            self.input_mode = true;
            self.input_buffer.clear();
        } else {
            // Select a preset
            let value = PRESETS[self.selected_idx];
            self.app_event_tx.send(AppEvent::SetConfigLoopCount(value));
            self.complete = true;
        }
    }

    fn submit_custom_input(&mut self) {
        if self.input_buffer.is_empty() {
            return;
        }
        if let Ok(n) = self.input_buffer.parse::<i32>() {
            let value = if n <= 1 {
                None
            } else if n > MAX_LOOP_COUNT {
                Some(MAX_LOOP_COUNT)
            } else {
                Some(n)
            };
            self.app_event_tx.send(AppEvent::SetConfigLoopCount(value));
            self.complete = true;
        }
        // If parse fails (shouldn't happen since we only accept digits), ignore.
    }

    fn cancel_input(&mut self) {
        self.input_mode = false;
        self.input_buffer.clear();
    }

    /// Display label for a given item index.
    fn item_label(&self, idx: usize) -> String {
        if idx == self.custom_idx() {
            if !self.current_is_preset()
                && let Some(n) = self.current
            {
                return format!("Custom... ({n})");
            }
            "Custom...".to_string()
        } else {
            match PRESETS[idx] {
                Some(n) => n.to_string(),
                None => "Disabled".to_string(),
            }
        }
    }

    /// Whether the item at `idx` represents the current value.
    fn is_item_current(&self, idx: usize) -> bool {
        if idx == self.custom_idx() {
            !self.current_is_preset()
        } else {
            PRESETS[idx] == self.current
        }
    }

    #[cfg(test)]
    pub(crate) fn selected_idx(&self) -> usize {
        self.selected_idx
    }

    #[cfg(test)]
    pub(crate) fn is_input_mode(&self) -> bool {
        self.input_mode
    }

    #[cfg(test)]
    pub(crate) fn input_buffer(&self) -> &str {
        &self.input_buffer
    }
}

impl BottomPaneView for LoopCountPickerView {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if key_event.kind != KeyEventKind::Press && key_event.kind != KeyEventKind::Repeat {
            return;
        }

        if self.input_mode {
            match key_event.code {
                KeyCode::Esc => {
                    self.cancel_input();
                }
                KeyCode::Enter => {
                    self.submit_custom_input();
                }
                KeyCode::Backspace => {
                    self.input_buffer.pop();
                }
                KeyCode::Char(c) if c.is_ascii_digit() => {
                    self.input_buffer.push(c);
                }
                _ => {
                    // Ignore non-digit characters
                }
            }
            return;
        }

        match key_event {
            KeyEvent {
                code: KeyCode::Up, ..
            }
            | KeyEvent {
                code: KeyCode::Char('k'),
                modifiers: KeyModifiers::NONE,
                ..
            } => self.move_up(),

            KeyEvent {
                code: KeyCode::Down,
                ..
            }
            | KeyEvent {
                code: KeyCode::Char('j'),
                modifiers: KeyModifiers::NONE,
                ..
            } => self.move_down(),

            KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::NONE,
                ..
            } => self.select_current(),

            KeyEvent {
                code: KeyCode::Esc, ..
            } => {
                self.on_ctrl_c();
            }

            _ => {}
        }
    }

    fn is_complete(&self) -> bool {
        self.complete
    }

    fn on_ctrl_c(&mut self) -> CancellationEvent {
        self.complete = true;
        CancellationEvent::Handled
    }
}

impl Renderable for LoopCountPickerView {
    fn desired_height(&self, _width: u16) -> u16 {
        // title + subtitle + blank + items + blank + footer hint + vertical inset (2)
        let content_rows = if self.input_mode {
            3 + 1 + 2 // title/subtitle/blank + input line + blank/footer
        } else {
            3 + self.item_count() + 2
        };
        (content_rows + 2) as u16
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        Block::default()
            .style(user_message_style())
            .render(area, buf);

        let content_area = area.inset(Insets::vh(1, 2));
        if content_area.height == 0 || content_area.width == 0 {
            return;
        }

        let mut constraints = vec![
            Constraint::Length(1), // title
            Constraint::Length(1), // subtitle
            Constraint::Length(1), // blank line
        ];

        if self.input_mode {
            constraints.push(Constraint::Length(1)); // input line
        } else {
            for _ in 0..self.item_count() {
                constraints.push(Constraint::Length(1));
            }
        }
        constraints.push(Constraint::Length(1)); // blank line
        constraints.push(Constraint::Length(1)); // footer hint

        let areas = Layout::vertical(constraints).split(content_area);
        let mut row = 0;

        // Title
        Line::from("Loop Count".bold()).render(areas[row], buf);
        row += 1;

        // Subtitle
        Line::from("Select number of loop iterations".dim()).render(areas[row], buf);
        row += 1;

        // Blank
        row += 1;

        if self.input_mode {
            // Input mode: show prompt with typed buffer
            let prompt = format!("Enter count (2-{MAX_LOOP_COUNT}): {}_", self.input_buffer);
            Line::from(prompt).render(areas[row], buf);
            row += 1;
        } else {
            // Normal mode: show items
            for idx in 0..self.item_count() {
                let is_selected = idx == self.selected_idx;
                let is_current = self.is_item_current(idx);
                let prefix = if is_selected { "› " } else { "  " };
                let label = self.item_label(idx);

                let line = if is_selected {
                    Line::from(vec![
                        prefix.to_string().bold(),
                        label.bold(),
                        if is_current { " ✓".dim() } else { "".into() },
                    ])
                } else {
                    Line::from(vec![
                        prefix.to_string().into(),
                        if is_current {
                            label.into()
                        } else {
                            label.dim()
                        },
                        if is_current { " ✓".dim() } else { "".into() },
                    ])
                };
                line.render(areas[row], buf);
                row += 1;
            }
        }

        // Blank
        row += 1;

        // Footer hint
        let hint = if self.input_mode {
            "enter submit · esc cancel"
        } else {
            "↑↓ select · enter choose · esc close"
        };
        Line::from(hint.dim()).render(areas[row], buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_event::AppEvent;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;
    use pretty_assertions::assert_eq;
    use tokio::sync::mpsc::unbounded_channel;

    fn make_picker(
        current: Option<i32>,
    ) -> (
        LoopCountPickerView,
        tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    ) {
        let (tx_raw, rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let picker = LoopCountPickerView::new(current, tx);
        (picker, rx)
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn press(picker: &mut LoopCountPickerView, code: KeyCode) {
        picker.handle_key_event(key(code, KeyModifiers::NONE));
    }

    fn type_digits(picker: &mut LoopCountPickerView, digits: &str) {
        for c in digits.chars() {
            press(picker, KeyCode::Char(c));
        }
    }

    #[test]
    fn preset_selection_sends_correct_event() {
        let (mut picker, mut rx) = make_picker(None);

        // Navigate to "5" (index 3: Disabled=0, 2=1, 3=2, 5=3)
        press(&mut picker, KeyCode::Down); // -> 2
        press(&mut picker, KeyCode::Down); // -> 3
        press(&mut picker, KeyCode::Down); // -> 5
        press(&mut picker, KeyCode::Enter);

        let event = rx.try_recv().expect("should receive event");
        match event {
            AppEvent::SetConfigLoopCount(value) => assert_eq!(value, Some(5)),
            _ => panic!("expected SetConfigLoopCount, got: {event:?}"),
        }
        assert!(picker.is_complete());
    }

    #[test]
    fn disabled_selection_sends_none() {
        let (mut picker, mut rx) = make_picker(Some(5));
        assert_eq!(picker.selected_idx(), 3);

        // Navigate up 3 times from index 3 to reach index 0 ("Disabled")
        press(&mut picker, KeyCode::Up); // -> 2
        press(&mut picker, KeyCode::Up); // -> 1
        press(&mut picker, KeyCode::Up); // -> 0
        assert_eq!(picker.selected_idx(), 0);
        press(&mut picker, KeyCode::Enter);

        let event = rx.try_recv().expect("should receive event");
        match event {
            AppEvent::SetConfigLoopCount(value) => assert_eq!(value, None),
            _ => panic!("expected SetConfigLoopCount(None), got: {event:?}"),
        }
    }

    #[test]
    fn custom_option_enters_input_mode() {
        let (mut picker, _rx) = make_picker(None);

        // Navigate to "Custom..." (last item, index 5)
        for _ in 0..5 {
            press(&mut picker, KeyCode::Down);
        }
        assert_eq!(picker.selected_idx(), 5);
        press(&mut picker, KeyCode::Enter);

        assert!(picker.is_input_mode());
        assert!(!picker.is_complete());
    }

    #[test]
    fn custom_input_submits_valid_number() {
        let (mut picker, mut rx) = make_picker(None);

        // Navigate to Custom and enter input mode
        for _ in 0..5 {
            press(&mut picker, KeyCode::Down);
        }
        press(&mut picker, KeyCode::Enter);
        assert!(picker.is_input_mode());

        // Type "25" and submit
        type_digits(&mut picker, "25");
        assert_eq!(picker.input_buffer(), "25");
        press(&mut picker, KeyCode::Enter);

        let event = rx.try_recv().expect("should receive event");
        match event {
            AppEvent::SetConfigLoopCount(value) => assert_eq!(value, Some(25)),
            _ => panic!("expected SetConfigLoopCount(Some(25)), got: {event:?}"),
        }
        assert!(picker.is_complete());
    }

    #[test]
    fn custom_input_rejects_non_digits() {
        let (mut picker, _rx) = make_picker(None);

        // Enter input mode
        for _ in 0..5 {
            press(&mut picker, KeyCode::Down);
        }
        press(&mut picker, KeyCode::Enter);

        // Type letters (should be ignored) then digits
        press(&mut picker, KeyCode::Char('a'));
        press(&mut picker, KeyCode::Char('b'));
        type_digits(&mut picker, "42");

        assert_eq!(picker.input_buffer(), "42");
    }

    #[test]
    fn custom_input_value_lte_1_sends_disabled() {
        let (mut picker, mut rx) = make_picker(None);

        // Enter input mode
        for _ in 0..5 {
            press(&mut picker, KeyCode::Down);
        }
        press(&mut picker, KeyCode::Enter);

        type_digits(&mut picker, "1");
        press(&mut picker, KeyCode::Enter);

        let event = rx.try_recv().expect("should receive event");
        match event {
            AppEvent::SetConfigLoopCount(value) => assert_eq!(value, None),
            _ => panic!("expected SetConfigLoopCount(None), got: {event:?}"),
        }
    }

    #[test]
    fn custom_input_caps_at_max() {
        let (mut picker, mut rx) = make_picker(None);

        // Enter input mode
        for _ in 0..5 {
            press(&mut picker, KeyCode::Down);
        }
        press(&mut picker, KeyCode::Enter);

        type_digits(&mut picker, "9999");
        press(&mut picker, KeyCode::Enter);

        let event = rx.try_recv().expect("should receive event");
        match event {
            AppEvent::SetConfigLoopCount(value) => assert_eq!(value, Some(MAX_LOOP_COUNT)),
            _ => panic!("expected SetConfigLoopCount(Some({MAX_LOOP_COUNT})), got: {event:?}"),
        }
    }

    #[test]
    fn esc_cancels_input_mode() {
        let (mut picker, mut rx) = make_picker(None);

        // Enter input mode
        for _ in 0..5 {
            press(&mut picker, KeyCode::Down);
        }
        press(&mut picker, KeyCode::Enter);
        assert!(picker.is_input_mode());

        type_digits(&mut picker, "50");
        press(&mut picker, KeyCode::Esc);

        assert!(!picker.is_input_mode());
        assert!(!picker.is_complete());
        // No event should have been sent
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn esc_closes_picker_when_not_in_input_mode() {
        let (mut picker, _rx) = make_picker(None);
        assert!(!picker.is_complete());

        press(&mut picker, KeyCode::Esc);
        assert!(picker.is_complete());
    }

    #[test]
    fn navigation_wraps_around() {
        let (mut picker, _rx) = make_picker(None);

        // Start at 0 (Disabled), go up should wrap to last item
        press(&mut picker, KeyCode::Up);
        assert_eq!(picker.selected_idx(), 5); // Custom...

        // Go down should wrap to first item
        press(&mut picker, KeyCode::Down);
        assert_eq!(picker.selected_idx(), 0); // Disabled
    }

    #[test]
    fn current_preset_value_is_highlighted() {
        let (picker, _rx) = make_picker(Some(5));
        // Some(5) is at index 3
        assert_eq!(picker.selected_idx(), 3);
    }

    #[test]
    fn non_preset_value_highlights_custom() {
        let (picker, _rx) = make_picker(Some(7));
        // Non-preset should highlight "Custom..." at index 5
        assert_eq!(picker.selected_idx(), 5);
    }

    #[test]
    fn backspace_removes_digit_in_input_mode() {
        let (mut picker, _rx) = make_picker(None);

        // Enter input mode
        for _ in 0..5 {
            press(&mut picker, KeyCode::Down);
        }
        press(&mut picker, KeyCode::Enter);

        type_digits(&mut picker, "123");
        assert_eq!(picker.input_buffer(), "123");

        press(&mut picker, KeyCode::Backspace);
        assert_eq!(picker.input_buffer(), "12");
    }

    #[test]
    fn empty_input_does_not_submit() {
        let (mut picker, mut rx) = make_picker(None);

        // Enter input mode
        for _ in 0..5 {
            press(&mut picker, KeyCode::Down);
        }
        press(&mut picker, KeyCode::Enter);

        // Press Enter with empty buffer
        press(&mut picker, KeyCode::Enter);

        assert!(!picker.is_complete());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn renders_without_panic() {
        let (picker, _rx) = make_picker(Some(5));
        let width = 60;
        let height = picker.desired_height(width);
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        picker.render(area, &mut buf);

        let text: String = (0..area.height)
            .map(|row| {
                (0..area.width)
                    .map(|col| {
                        let s = buf[(col, row)].symbol();
                        if s.is_empty() { " " } else { s }
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("Loop Count"), "should contain title");
        assert!(text.contains("Disabled"), "should contain Disabled option");
        assert!(text.contains("Custom"), "should contain Custom option");
    }

    #[test]
    fn renders_input_mode_without_panic() {
        let (mut picker, _rx) = make_picker(None);

        // Enter input mode
        for _ in 0..5 {
            press(&mut picker, KeyCode::Down);
        }
        press(&mut picker, KeyCode::Enter);
        type_digits(&mut picker, "42");

        let width = 60;
        let height = picker.desired_height(width);
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        picker.render(area, &mut buf);

        let text: String = (0..area.height)
            .map(|row| {
                (0..area.width)
                    .map(|col| {
                        let s = buf[(col, row)].symbol();
                        if s.is_empty() { " " } else { s }
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("42"), "should show typed digits");
        assert!(text.contains("submit"), "should show submit hint");
    }

    #[test]
    fn jk_navigation_works() {
        let (mut picker, _rx) = make_picker(None);
        assert_eq!(picker.selected_idx(), 0);

        press(&mut picker, KeyCode::Char('j'));
        assert_eq!(picker.selected_idx(), 1);

        press(&mut picker, KeyCode::Char('k'));
        assert_eq!(picker.selected_idx(), 0);
    }
}
