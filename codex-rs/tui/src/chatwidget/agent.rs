use std::sync::Arc;

use codex_acp::AcpBackend;
use codex_acp::AcpBackendConfig;
use codex_acp::get_agent_config;
use codex_core::CodexConversation;
use codex_core::ConversationManager;
use codex_core::NewConversation;
use codex_core::config::Config;
use codex_core::protocol::Event;
use codex_core::protocol::EventMsg;
use codex_core::protocol::Op;
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::mpsc::unbounded_channel;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;

/// Spawn the agent bootstrapper and op forwarding loop, returning the
/// `UnboundedSender<Op>` used by the UI to submit operations.
///
/// This function detects whether to use ACP mode or HTTP mode based on
/// whether the model name matches an ACP agent in the registry.
pub(crate) fn spawn_agent(
    config: Config,
    app_event_tx: AppEventSender,
    server: Arc<ConversationManager>,
) -> UnboundedSender<Op> {
    // Detect ACP mode based on model name
    if get_agent_config(&config.model).is_ok() {
        spawn_acp_agent(config, app_event_tx)
    } else {
        spawn_http_agent(config, app_event_tx, server)
    }
}

/// Spawn an ACP agent backend.
///
/// This uses the `codex_acp` crate to spawn an agent subprocess and handle
/// communication via the Agent Client Protocol.
fn spawn_acp_agent(config: Config, app_event_tx: AppEventSender) -> UnboundedSender<Op> {
    let (codex_op_tx, mut codex_op_rx) = unbounded_channel::<Op>();

    tokio::spawn(async move {
        // Create event channel for backend → TUI
        let (event_tx, mut event_rx) = mpsc::channel(32);

        // Create ACP backend config from codex config
        let acp_config = AcpBackendConfig {
            model: config.model.clone(),
            cwd: config.cwd.clone(),
            approval_policy: config.approval_policy,
            sandbox_policy: config.sandbox_policy.clone(),
        };

        let backend = match AcpBackend::spawn(&acp_config, event_tx).await {
            Ok(b) => Arc::new(b),
            Err(e) => {
                tracing::error!("failed to spawn ACP backend: {e}");
                app_event_tx.send(AppEvent::CodexEvent(Event {
                    id: String::new(),
                    msg: EventMsg::Error(codex_protocol::protocol::ErrorEvent {
                        message: format!("Failed to spawn ACP agent: {e}"),
                        codex_error_info: None,
                    }),
                }));
                app_event_tx.send(AppEvent::ExitRequest);
                return;
            }
        };

        // Forward ops to backend
        let backend_for_ops = Arc::clone(&backend);
        tokio::spawn(async move {
            while let Some(op) = codex_op_rx.recv().await {
                if let Err(e) = backend_for_ops.submit(op).await {
                    tracing::error!("failed to submit op: {e}");
                }
            }
        });

        // Forward events to TUI
        while let Some(event) = event_rx.recv().await {
            app_event_tx.send(AppEvent::CodexEvent(event));
        }
    });

    codex_op_tx
}

/// Spawn an HTTP agent (the original implementation).
///
/// This uses `codex_core` to communicate with LLM providers via HTTP APIs.
fn spawn_http_agent(
    config: Config,
    app_event_tx: AppEventSender,
    server: Arc<ConversationManager>,
) -> UnboundedSender<Op> {
    let (codex_op_tx, mut codex_op_rx) = unbounded_channel::<Op>();

    let app_event_tx_clone = app_event_tx;
    tokio::spawn(async move {
        let NewConversation {
            conversation_id: _,
            conversation,
            session_configured,
        } = match server.new_conversation(config).await {
            Ok(v) => v,
            #[allow(clippy::print_stderr)]
            Err(err) => {
                let message = err.to_string();
                eprintln!("{message}");
                app_event_tx_clone.send(AppEvent::CodexEvent(Event {
                    id: "".to_string(),
                    msg: EventMsg::Error(err.to_error_event(None)),
                }));
                app_event_tx_clone.send(AppEvent::ExitRequest);
                tracing::error!("failed to initialize codex: {err}");
                return;
            }
        };

        // Forward the captured `SessionConfigured` event so it can be rendered in the UI.
        let ev = codex_core::protocol::Event {
            // The `id` does not matter for rendering, so we can use a fake value.
            id: "".to_string(),
            msg: codex_core::protocol::EventMsg::SessionConfigured(session_configured),
        };
        app_event_tx_clone.send(AppEvent::CodexEvent(ev));

        let conversation_clone = conversation.clone();
        tokio::spawn(async move {
            while let Some(op) = codex_op_rx.recv().await {
                let id = conversation_clone.submit(op).await;
                if let Err(e) = id {
                    tracing::error!("failed to submit op: {e}");
                }
            }
        });

        while let Ok(event) = conversation.next_event().await {
            app_event_tx_clone.send(AppEvent::CodexEvent(event));
        }
    });

    codex_op_tx
}

/// Spawn agent loops for an existing conversation (e.g., a forked conversation).
/// Sends the provided `SessionConfiguredEvent` immediately, then forwards subsequent
/// events and accepts Ops for submission.
pub(crate) fn spawn_agent_from_existing(
    conversation: std::sync::Arc<CodexConversation>,
    session_configured: codex_core::protocol::SessionConfiguredEvent,
    app_event_tx: AppEventSender,
) -> UnboundedSender<Op> {
    let (codex_op_tx, mut codex_op_rx) = unbounded_channel::<Op>();

    let app_event_tx_clone = app_event_tx;
    tokio::spawn(async move {
        // Forward the captured `SessionConfigured` event so it can be rendered in the UI.
        let ev = codex_core::protocol::Event {
            id: "".to_string(),
            msg: codex_core::protocol::EventMsg::SessionConfigured(session_configured),
        };
        app_event_tx_clone.send(AppEvent::CodexEvent(ev));

        let conversation_clone = conversation.clone();
        tokio::spawn(async move {
            while let Some(op) = codex_op_rx.recv().await {
                let id = conversation_clone.submit(op).await;
                if let Err(e) = id {
                    tracing::error!("failed to submit op: {e}");
                }
            }
        });

        while let Ok(event) = conversation.next_event().await {
            app_event_tx_clone.send(AppEvent::CodexEvent(event));
        }
    });

    codex_op_tx
}
