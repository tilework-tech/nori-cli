use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::DateTime;
use chrono::Utc;
use codex_core::ConversationItem;
use codex_core::ConversationsPage;
use codex_core::Cursor;
use codex_core::INTERACTIVE_SESSION_SOURCES;
use codex_core::RolloutRecorder;
use codex_protocol::items::TurnItem;
use color_eyre::eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Stylize as _;
use ratatui::text::Line;
use ratatui::text::Span;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::UnboundedReceiverStream;
use unicode_width::UnicodeWidthStr;

use crate::diff_render::display_path_for;
use crate::key_hint;
use crate::text_formatting::truncate_text;
use crate::tui::FrameRequester;
use crate::tui::Tui;
use crate::tui::TuiEvent;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::SessionMetaLine;

mod helpers;
mod rendering;
mod state;
#[cfg(test)]
mod tests;

const PAGE_SIZE: usize = 25;
const LOAD_NEAR_THRESHOLD: usize = 5;

#[derive(Debug, Clone)]
pub enum ResumeSelection {
    StartFresh,
    Resume(PathBuf),
    Exit,
}

#[derive(Clone)]
struct PageLoadRequest {
    pub(super) codex_home: PathBuf,
    pub(super) cursor: Option<Cursor>,
    pub(super) request_token: usize,
    pub(super) search_token: Option<usize>,
    pub(super) default_provider: String,
}

type PageLoader = Arc<dyn Fn(PageLoadRequest) + Send + Sync>;

enum BackgroundEvent {
    PageLoaded {
        request_token: usize,
        search_token: Option<usize>,
        page: std::io::Result<ConversationsPage>,
    },
}

/// Interactive session picker that lists recorded rollout files with simple
/// search and pagination. Shows the first user input as the preview, relative
/// time (e.g., "5 seconds ago"), and the absolute path.
pub async fn run_resume_picker(
    tui: &mut Tui,
    codex_home: &Path,
    default_provider: &str,
    show_all: bool,
) -> Result<ResumeSelection> {
    let alt = AltScreenGuard::enter(tui);
    let (bg_tx, bg_rx) = mpsc::unbounded_channel();

    let default_provider = default_provider.to_string();
    let filter_cwd = if show_all {
        None
    } else {
        std::env::current_dir().ok()
    };

    let loader_tx = bg_tx.clone();
    let page_loader: PageLoader = Arc::new(move |request: PageLoadRequest| {
        let tx = loader_tx.clone();
        tokio::spawn(async move {
            let provider_filter = vec![request.default_provider.clone()];
            let page = RolloutRecorder::list_conversations(
                &request.codex_home,
                PAGE_SIZE,
                request.cursor.as_ref(),
                INTERACTIVE_SESSION_SOURCES,
                Some(provider_filter.as_slice()),
                request.default_provider.as_str(),
            )
            .await;
            let _ = tx.send(BackgroundEvent::PageLoaded {
                request_token: request.request_token,
                search_token: request.search_token,
                page,
            });
        });
    });

    let mut state = PickerState::new(
        codex_home.to_path_buf(),
        alt.tui.frame_requester(),
        page_loader,
        default_provider.clone(),
        show_all,
        filter_cwd,
    );
    state.load_initial_page().await?;
    state.request_frame();

    let mut tui_events = alt.tui.event_stream().fuse();
    let mut background_events = UnboundedReceiverStream::new(bg_rx).fuse();

    loop {
        tokio::select! {
            Some(ev) = tui_events.next() => {
                match ev {
                    TuiEvent::Key(key) => {
                        if matches!(key.kind, KeyEventKind::Release) {
                            continue;
                        }
                        if let Some(sel) = state.handle_key(key).await? {
                            return Ok(sel);
                        }
                    }
                    TuiEvent::Draw => {
                        if let Ok(size) = alt.tui.terminal.size() {
                            let list_height = size.height.saturating_sub(4) as usize;
                            state.update_view_rows(list_height);
                            state.ensure_minimum_rows_for_view(list_height);
                        }
                        rendering::draw_picker(alt.tui, &state)?;
                    }
                    _ => {}
                }
            }
            Some(event) = background_events.next() => {
                state.handle_background_event(event)?;
            }
            else => break,
        }
    }

    // Fallback – treat as cancel/new
    Ok(ResumeSelection::StartFresh)
}

/// RAII guard that ensures we leave the alt-screen on scope exit.
struct AltScreenGuard<'a> {
    tui: &'a mut Tui,
}

impl<'a> AltScreenGuard<'a> {
    fn enter(tui: &'a mut Tui) -> Self {
        let _ = tui.enter_alt_screen();
        Self { tui }
    }
}

impl Drop for AltScreenGuard<'_> {
    fn drop(&mut self) {
        let _ = self.tui.leave_alt_screen();
    }
}

struct PickerState {
    pub(super) codex_home: PathBuf,
    pub(super) requester: FrameRequester,
    pub(super) pagination: PaginationState,
    pub(super) all_rows: Vec<Row>,
    pub(super) filtered_rows: Vec<Row>,
    pub(super) seen_paths: HashSet<PathBuf>,
    pub(super) selected: usize,
    pub(super) scroll_top: usize,
    pub(super) query: String,
    pub(super) search_state: SearchState,
    pub(super) next_request_token: usize,
    pub(super) next_search_token: usize,
    pub(super) page_loader: PageLoader,
    pub(super) view_rows: Option<usize>,
    pub(super) default_provider: String,
    pub(super) show_all: bool,
    pub(super) filter_cwd: Option<PathBuf>,
}

struct PaginationState {
    pub(super) next_cursor: Option<Cursor>,
    pub(super) num_scanned_files: usize,
    pub(super) reached_scan_cap: bool,
    pub(super) loading: LoadingState,
}

#[derive(Clone, Copy, Debug)]
enum LoadingState {
    Idle,
    Pending(PendingLoad),
}

#[derive(Clone, Copy, Debug)]
struct PendingLoad {
    pub(super) request_token: usize,
    pub(super) search_token: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
enum SearchState {
    Idle,
    Active { token: usize },
}

enum LoadTrigger {
    Scroll,
    Search { token: usize },
}

impl LoadingState {
    pub(super) fn is_pending(&self) -> bool {
        matches!(self, LoadingState::Pending(_))
    }
}

impl SearchState {
    pub(super) fn active_token(&self) -> Option<usize> {
        match self {
            SearchState::Idle => None,
            SearchState::Active { token } => Some(*token),
        }
    }

    pub(super) fn is_active(&self) -> bool {
        self.active_token().is_some()
    }
}

#[derive(Clone)]
struct Row {
    pub(super) path: PathBuf,
    pub(super) preview: String,
    pub(super) created_at: Option<DateTime<Utc>>,
    pub(super) updated_at: Option<DateTime<Utc>>,
    pub(super) cwd: Option<PathBuf>,
    pub(super) git_branch: Option<String>,
}
