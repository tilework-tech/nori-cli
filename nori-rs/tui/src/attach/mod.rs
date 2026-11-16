//! A terminal attachment observes host-owned sessions without owning their lifetime.

mod connection;
#[cfg(test)]
mod connection_tests;
mod input;
mod render;
mod runtime;
mod state;
#[cfg(test)]
mod tests;

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
    anyhow::ensure!(
        config.multi_session,
        "nori attach requires [features] multi_session = true"
    );
    let initial = ManagedClient::connect(&options.endpoint, &options.token).await?;
    let mut tui = crate::tui::Tui::new(crate::tui::init()?);
    let result = async {
        tui.enter_alt_screen()?;
        run(&mut tui, options, initial, config.hotkeys).await
    }
    .await;
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
    hotkeys: nori_config::HotkeyConfig,
) -> anyhow::Result<()> {
    let (request_tx, request_rx) = tokio::sync::mpsc::unbounded_channel();
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = runtime::Runtime::new(options.session_id.clone(), request_tx);
    runtime.set_session_list_binding(hotkeys.open_session_list);
    runtime.set_next_waiting_binding(hotkeys.next_waiting_session);
    let connection = tokio::spawn(connection::drive(options, initial, request_rx, event_tx));
    let mut events = tui.event_stream();
    let mut refresh = tokio::time::interval(std::time::Duration::from_secs(1));
    let result = async {
        loop {
            tokio::select! {
                incoming = event_rx.recv(), if runtime.state.stopped.is_none() => {
                    let Some(incoming) = incoming else { anyhow::bail!("Host connection worker stopped"); };
                    if let Err(error) = runtime.connection_event(incoming) {
                        runtime.state.show_notice(error.to_string());
                    }
                    if runtime.state.stopped.is_some() {
                        connection.abort();
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
            runtime.composer.flush_paste_burst_if_due();
            let height = tui.terminal.size()?.height;
            tui.draw(height, |frame| {
                let area = frame.area();
                render::render(&runtime.state, &runtime.composer, area, frame.buffer_mut());
                if let Some(position) = runtime.composer.cursor_pos(render::composer_area(&runtime.state, &runtime.composer, area)) {
                    frame.set_cursor_position(position);
                }
            })?;
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
