use super::*;

impl ChatComposer {
    pub(super) fn layout_areas(&self, area: Rect) -> [Rect; 3] {
        let footer_props = self.footer_props();
        let footer_hint_height = self
            .custom_footer_height()
            .unwrap_or_else(|| footer_height(&footer_props));
        let footer_spacing = Self::footer_spacing(footer_hint_height);
        let footer_total_height = footer_hint_height + footer_spacing;
        let popup_constraint = match &self.active_popup {
            ActivePopup::Command(popup) => {
                Constraint::Max(popup.calculate_required_height(area.width))
            }
            ActivePopup::File(popup) => Constraint::Max(popup.calculate_required_height()),
            ActivePopup::HistorySearch(popup) => Constraint::Max(popup.calculate_required_height()),
            ActivePopup::None => Constraint::Max(footer_total_height),
        };
        let [composer_rect, popup_rect] =
            Layout::vertical([Constraint::Min(3), popup_constraint]).areas(area);
        let textarea_rect = composer_rect.inset(Insets::tlbr(1, LIVE_PREFIX_COLS, 1, 1));
        [composer_rect, textarea_rect, popup_rect]
    }

    pub(super) fn footer_spacing(footer_hint_height: u16) -> u16 {
        if footer_hint_height == 0 {
            0
        } else {
            FOOTER_SPACING_HEIGHT
        }
    }

    pub(super) fn footer_props(&self) -> FooterProps {
        let (
            git_branch,
            active_skillsets,
            nori_version,
            nori_version_source,
            git_lines_added,
            git_lines_removed,
        ) = if let Some(ref info) = self.system_info {
            (
                info.git_branch.clone(),
                info.active_skillsets.clone(),
                info.nori_version.clone(),
                info.nori_version_source,
                info.git_lines_added,
                info.git_lines_removed,
            )
        } else {
            (None, Vec::new(), None, None, None, None)
        };

        // Extract token breakdown and agent kind from transcript location
        let transcript_location = self
            .system_info
            .as_ref()
            .and_then(|s| s.transcript_location.as_ref());
        let token_breakdown = transcript_location.and_then(|loc| loc.token_breakdown.as_ref());
        let context_window_size =
            transcript_location.map(|loc| loc.agent_kind.context_window_size());
        // Use last_context_tokens (input-side tokens from the most recent
        // main-chain message) for context window fill display and percentage.
        // Falls back to cumulative total() when last_context_tokens is not
        // available (e.g., Gemini).
        let context_tokens = token_breakdown.and_then(|t| {
            t.last_context_tokens
                .or_else(|| Some(t.total()))
                .filter(|&v| v > 0)
        });
        let context_window_percent = self.context_window_percent.or_else(|| {
            context_window_size.and_then(|window_size| {
                context_tokens.map(|tokens| {
                    if window_size > 0 {
                        ((tokens as f64 / window_size as f64) * 100.0).round() as i64
                    } else {
                        0
                    }
                })
            })
        });
        let (context_tokens, context_window_percent) =
            if let Some(session_usage) = &self.session_usage {
                (
                    Some(session_usage.used_tokens).filter(|&tokens| tokens > 0),
                    (session_usage.total_tokens > 0).then(|| {
                        session_usage
                            .used_tokens
                            .saturating_mul(100)
                            .saturating_div(session_usage.total_tokens)
                            .clamp(0, 100)
                    }),
                )
            } else {
                (context_tokens, context_window_percent)
            };

        FooterProps {
            mode: self.footer_mode(),
            esc_backtrack_hint: self.esc_backtrack_hint,
            use_shift_enter_hint: self.use_shift_enter_hint,
            is_task_running: self.is_task_running,
            vertical_footer: self.vertical_footer,
            context_window_percent,
            context_tokens,
            git_branch,
            approval_mode_label: self.approval_mode_label.clone(),
            active_skillsets,
            nori_version,
            nori_version_source,
            git_lines_added,
            git_lines_removed,
            is_worktree: self
                .system_info
                .as_ref()
                .map(|s| s.is_worktree)
                .unwrap_or(false),
            input_tokens: token_breakdown.map(|t| t.input_tokens),
            output_tokens: token_breakdown.map(|t| t.output_tokens),
            cached_tokens: token_breakdown.map(|t| t.cached_tokens),
            vim_mode_state: self.textarea.vim_mode_state_if_enabled(),
            prompt_summary: self.prompt_summary.clone(),
            worktree_name: self
                .system_info
                .as_ref()
                .and_then(|s| s.worktree_name.clone()),
            footer_segment_config: self.footer_segment_config.clone(),
        }
    }

    pub(super) fn footer_mode(&self) -> FooterMode {
        match self.footer_mode {
            FooterMode::EscHint => FooterMode::EscHint,
            FooterMode::ShortcutOverlay => FooterMode::ShortcutOverlay,
            FooterMode::CtrlCReminder => FooterMode::CtrlCReminder,
            FooterMode::ShortcutSummary if self.ctrl_c_quit_hint => FooterMode::CtrlCReminder,
            FooterMode::ShortcutSummary if !self.is_empty() => FooterMode::ContextOnly,
            other => other,
        }
    }

    pub(super) fn custom_footer_height(&self) -> Option<u16> {
        self.footer_hint_override
            .as_ref()
            .map(|items| if items.is_empty() { 0 } else { 1 })
    }
}
