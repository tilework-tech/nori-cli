//! A terminal attachment observes host-owned sessions without owning their lifetime.

mod approvals;
mod close;
mod commands;
mod input;
mod new_session;
mod render;
mod replies;
mod runtime;
mod state;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_thoughts;

use nori_harness::remote_agent::ManagedClient;
use tokio_stream::StreamExt;

/// Connection parameters resolved by the CLI before entering the terminal.
/// The token is deliberately excluded from Debug output.
pub struct AttachOptions {
    pub endpoint: String,
    pub token: String,
    pub session_id: Option<String>,
    /// Secure local discovery only; explicit remote credentials are never refreshed.
    pub refresh_local_token: Option<Box<dyn Fn() -> anyhow::Result<String> + Send + Sync>>,
}

/// Attach to an existing host. Leaving this frontend never stops host sessions.
pub async fn run_attach(
    config: nori_config::NoriConfig,
    options: AttachOptions,
) -> anyhow::Result<()> {
    let initial = ManagedClient::connect(&options.endpoint, &options.token).await?;
    let mut tui = crate::tui::Tui::new(crate::tui::init()?);
    let mut program_status = crate::program_status::ProgramStatusReporter::from_config(&config);
    let result = async {
        tui.enter_alt_screen()?;
        run(&mut tui, options, initial, config, &mut program_status).await
    }
    .await;
    if let Some(reporter) = program_status.as_mut()
        && let Err(error) = reporter.clear_terminal()
    {
        tracing::debug!(%error, "could not clear program status");
    }
    let leave = tui.leave_alt_screen();
    let restore = crate::tui::restore();
    result?;
    leave?;
    restore?;
    Ok(())
}

async fn run(
    tui: &mut crate::tui::Tui,
    options: AttachOptions,
    initial: ManagedClient,
    config: nori_config::NoriConfig,
    program_status: &mut Option<crate::program_status::ProgramStatusReporter>,
) -> anyhow::Result<()> {
    let hotkeys = config.hotkeys;
    let (request_tx, request_rx) = tokio::sync::mpsc::unbounded_channel();
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = runtime::Runtime::new(options.session_id.clone(), request_tx);
    runtime.cwd = config.cwd;
    runtime.state.thoughts_in_history = config.thoughts_in_history;
    runtime.remote = options.refresh_local_token.is_none();
    runtime.set_session_list_binding(hotkeys.open_session_list);
    runtime.set_next_waiting_binding(hotkeys.next_waiting_session);
    runtime.state.parent_binding = hotkeys.parent_session;
    let endpoint = crate::host_client::HostEndpoint {
        endpoint: options.endpoint,
        token: options.token,
        refresh_local_token: options.refresh_local_token,
    };
    let connection = tokio::spawn(crate::host_client::drive(
        endpoint, initial, request_rx, event_tx,
    ));
    let mut events = tui.event_stream();
    let mut refresh = tokio::time::interval(std::time::Duration::from_secs(1));
    // A subscription can deliver retained events faster than the terminal can paint.
    // Bound each batch so keyboard input and list refreshes still get a turn.
    const EVENTS_PER_FRAME: usize = 64;
    let mut painted_request = None;
    let mut incoming_events = Vec::with_capacity(EVENTS_PER_FRAME);
    let result = async {
        loop {
            tokio::select! {
                count = event_rx.recv_many(&mut incoming_events, EVENTS_PER_FRAME), if runtime.state.stopped.is_none() => {
                    if count == 0 { anyhow::bail!("Host connection worker stopped"); }
                    for incoming in incoming_events.drain(..) {
                        if let Err(error) = runtime.connection_event(incoming) {
                            runtime.state.show_notice(error.to_string());
                        }
                        if runtime.state.stopped.is_some() {
                            connection.abort();
                            break;
                        }
                    }
                }
                event = events.next() => {
                    match event {
                        Some(crate::tui::TuiEvent::Key(key)) => {
                            if !runtime.key(key) { break; }
                        }
                        Some(crate::tui::TuiEvent::Paste(text)) => { runtime.composer.handle_paste(text); }
                        Some(crate::tui::TuiEvent::Draw) => {}
                        None => break,
                    }
                }
                _ = refresh.tick() => runtime.refresh_list(),
            }
            // Replayed history arrives as a burst; report once it has landed.
            // A stopped connection delivers nothing more, so report at once.
            if (event_rx.is_empty() || runtime.state.stopped.is_some())
                && let Some(reporter) = program_status.as_mut()
                && let Some(status) = runtime.state.foreground_program_status()
                && let Err(error) = reporter.report_to_terminal(&status)
            {
                tracing::debug!(%error, "could not report program status");
            }
            runtime.composer.flush_paste_burst_if_due();
            runtime.search_files();
            let height = tui.terminal.size()?.height;
            tui.draw(height, |frame| {
                let area = frame.area();
                render::render(&runtime.state, &runtime.composer, area, frame.buffer_mut());
                if let Some(position) = render::cursor_position(&runtime.state, &runtime.composer, area) {
                    frame.set_cursor_position(position);
                }
            })?;
            runtime.request_painted(&mut painted_request);
            if runtime.composer.is_in_paste_burst() {
                tui.frame_requester().schedule_frame_in(crate::ComposerInput::recommended_flush_delay());
            }
        }
        Ok(())
    }.await;
    connection.abort();
    let _ = connection.await;
    result
}
