//! Custom model input: a `BottomPaneView` that accepts a free-form model ID
//! string. Opened from the model picker's "Use custom model..." entry.

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
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

pub(crate) struct CustomModelInputView {
    config_id: String,
    option_name: String,
    input_buffer: String,
    complete: bool,
    app_event_tx: AppEventSender,
}

impl CustomModelInputView {
    pub fn new(config_id: String, option_name: String, app_event_tx: AppEventSender) -> Self {
        Self {
            config_id,
            option_name,
            input_buffer: String::new(),
            complete: false,
            app_event_tx,
        }
    }

    fn submit(&mut self) {
        let value = self.input_buffer.trim().to_string();
        if value.is_empty() {
            return;
        }
        self.app_event_tx.send(AppEvent::SetAcpSessionConfigOption {
            config_id: self.config_id.clone(),
            value: value.clone(),
            option_name: self.option_name.clone(),
            value_name: value,
        });
        self.complete = true;
    }
}

impl BottomPaneView for CustomModelInputView {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if key_event.kind != KeyEventKind::Press && key_event.kind != KeyEventKind::Repeat {
            return;
        }

        match key_event.code {
            KeyCode::Esc => {
                self.on_ctrl_c();
            }
            KeyCode::Enter => {
                self.submit();
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            KeyCode::Char(c) => {
                self.input_buffer.push(c);
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

    fn handle_paste(&mut self, pasted: String) -> bool {
        self.input_buffer.push_str(&pasted);
        true
    }
}

impl Renderable for CustomModelInputView {
    fn desired_height(&self, _width: u16) -> u16 {
        // title + subtitle + blank + input line + blank + footer + vertical inset (2)
        8
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let insets = Insets::vh(1, 2);
        let inner = area.inset(insets);

        Block::bordered()
            .title(" Custom Model ")
            .border_style(user_message_style())
            .render(area, buf);

        let chunks = Layout::vertical([
            Constraint::Length(1), // subtitle
            Constraint::Length(1), // blank
            Constraint::Length(1), // input
            Constraint::Length(1), // blank
            Constraint::Length(1), // hint
        ])
        .split(inner);

        Line::from("Enter a model ID (e.g. claude-opus-4-8)")
            .dim()
            .render(chunks[0], buf);

        let input_line = if self.input_buffer.is_empty() {
            Line::from(vec!["› ".into(), "│".dim()])
        } else {
            Line::from(format!("› {}│", self.input_buffer))
        };
        input_line.render(chunks[2], buf);

        Line::from("Enter to submit · Esc to cancel")
            .dim()
            .render(chunks[4], buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_input() -> (
        CustomModelInputView,
        tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let sender = AppEventSender::new(tx);
        let view = CustomModelInputView::new("model".to_string(), "Model".to_string(), sender);
        (view, rx)
    }

    fn press(view: &mut CustomModelInputView, code: KeyCode) {
        view.handle_key_event(KeyEvent::new(code, crossterm::event::KeyModifiers::NONE));
    }

    fn type_str(view: &mut CustomModelInputView, s: &str) {
        for c in s.chars() {
            press(view, KeyCode::Char(c));
        }
    }

    #[test]
    fn typing_and_submitting_emits_set_config_event() {
        let (mut view, mut rx) = make_input();

        type_str(&mut view, "claude-opus-4-8");
        press(&mut view, KeyCode::Enter);
        assert!(view.is_complete());

        let event = rx.try_recv().expect("should emit event");
        assert!(matches!(
            event,
            AppEvent::SetAcpSessionConfigOption {
                config_id,
                value,
                option_name,
                value_name,
            } if config_id == "model"
                && value == "claude-opus-4-8"
                && option_name == "Model"
                && value_name == "claude-opus-4-8"
        ));
    }

    #[test]
    fn empty_input_does_not_submit() {
        let (mut view, mut rx) = make_input();

        press(&mut view, KeyCode::Enter);
        assert!(!view.is_complete());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn whitespace_only_input_does_not_submit() {
        let (mut view, mut rx) = make_input();

        type_str(&mut view, "   ");
        press(&mut view, KeyCode::Enter);
        assert!(!view.is_complete());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn escape_dismisses_without_emitting() {
        let (mut view, mut rx) = make_input();

        type_str(&mut view, "some-model");
        press(&mut view, KeyCode::Esc);

        assert!(view.is_complete());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn backspace_removes_last_character() {
        let (mut view, mut rx) = make_input();

        type_str(&mut view, "abc");
        press(&mut view, KeyCode::Backspace);
        press(&mut view, KeyCode::Enter);

        let event = rx.try_recv().expect("should emit event");
        assert!(matches!(
            event,
            AppEvent::SetAcpSessionConfigOption { value, .. } if value == "ab"
        ));
    }

    #[test]
    fn accepts_any_characters() {
        let (mut view, mut rx) = make_input();

        type_str(&mut view, "my-custom/model_v2.1");
        press(&mut view, KeyCode::Enter);

        let event = rx.try_recv().expect("should emit event");
        assert!(matches!(
            event,
            AppEvent::SetAcpSessionConfigOption { value, .. } if value == "my-custom/model_v2.1"
        ));
    }

    #[test]
    fn trims_whitespace_on_submit() {
        let (mut view, mut rx) = make_input();

        type_str(&mut view, "  claude-opus-4-8  ");
        press(&mut view, KeyCode::Enter);

        let event = rx.try_recv().expect("should emit event");
        assert!(matches!(
            event,
            AppEvent::SetAcpSessionConfigOption { value, .. } if value == "claude-opus-4-8"
        ));
    }

    #[test]
    fn paste_appends_to_input_buffer() {
        let (mut view, mut rx) = make_input();

        type_str(&mut view, "claude-");
        view.handle_paste("opus-4-8".to_string());
        press(&mut view, KeyCode::Enter);

        let event = rx.try_recv().expect("should emit event");
        assert!(matches!(
            event,
            AppEvent::SetAcpSessionConfigOption { value, .. } if value == "claude-opus-4-8"
        ));
    }

    #[test]
    fn snapshot_empty_state() {
        use crate::render::renderable::Renderable;
        use crate::test_backend::VT100Backend;
        use ratatui::Terminal;

        let (view, _rx) = make_input();
        let mut terminal = Terminal::new(VT100Backend::new(50, 8)).expect("terminal");
        terminal
            .draw(|frame| {
                view.render(frame.area(), frame.buffer_mut());
            })
            .expect("render");
        insta::assert_snapshot!(terminal.backend());
    }

    #[test]
    fn snapshot_with_typed_input() {
        use crate::render::renderable::Renderable;
        use crate::test_backend::VT100Backend;
        use ratatui::Terminal;

        let (mut view, _rx) = make_input();
        type_str(&mut view, "claude-opus-4-8");
        let mut terminal = Terminal::new(VT100Backend::new(50, 8)).expect("terminal");
        terminal
            .draw(|frame| {
                view.render(frame.area(), frame.buffer_mut());
            })
            .expect("render");
        insta::assert_snapshot!(terminal.backend());
    }
}
