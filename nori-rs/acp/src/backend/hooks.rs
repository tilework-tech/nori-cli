use super::*;

/// Spawn a separate ACP connection, send a summarization prompt, and emit a
/// `PromptSummary` event with the result. Designed to be called as a
/// fire-and-forget task from `handle_user_input`.
pub(super) async fn run_prompt_summary(
    event_tx: &mpsc::Sender<Event>,
    agent_name: &str,
    cwd: &std::path::Path,
    user_prompt: &str,
    auto_worktree: crate::config::AutoWorktree,
    auto_worktree_repo_root: Option<&std::path::Path>,
    acp_proxy: crate::config::AcpProxyConfig,
) -> Result<()> {
    use tokio::time::Duration;
    use tokio::time::timeout;

    let agent_config = get_agent_config(agent_name)?;
    let mut connection = SacpConnection::spawn(&agent_config, cwd, acp_proxy).await?;
    let session_id = connection.create_session(cwd, vec![]).await?;

    let summarization_prompt = format!(
        "Summarize the following user request in 5 words or fewer. \
         Reply with ONLY the summary, no extra text.\n\n{user_prompt}"
    );
    let prompt = vec![translator::text_to_content_block(&summarization_prompt)];

    // Take the ordered event receiver so we can collect updates from this
    // throwaway connection. The main session uses the reducer loop instead.
    let mut event_rx = connection.take_event_receiver();

    // Consume updates in a task to accumulate the agent's text response
    let collector = tokio::spawn(async move {
        let mut text = String::new();
        while let Some(event) = event_rx.recv().await {
            if let crate::connection::ConnectionEvent::SessionUpdate(
                acp::SessionUpdate::AgentMessageChunk(chunk),
            ) = &event
                && let acp::ContentBlock::Text(t) = &chunk.content
            {
                text.push_str(&t.text);
            }
        }
        text
    });

    // Send the prompt with a timeout to prevent indefinite hangs
    let prompt_result = timeout(
        Duration::from_secs(30),
        connection.prompt(session_id, prompt),
    )
    .await;

    // Drop the connection to clean up the subprocess.
    drop(connection);

    match prompt_result {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => return Err(e),
        Err(_) => {
            debug!("Prompt summary timed out");
            return Ok(());
        }
    }

    let mut summary = collector.await.unwrap_or_default().trim().to_string();
    // Truncate to prevent a runaway response from dominating the footer
    if summary.chars().count() > 40 {
        summary = summary.chars().take(37).collect::<String>();
        summary.push_str("...");
    }
    if !summary.is_empty() {
        // If auto_worktree is enabled, rename the branch based on the summary.
        // Only the branch is renamed; the directory stays unchanged so that
        // processes running inside the worktree are not disrupted.
        if auto_worktree.is_enabled()
            && let Some(repo_root) = auto_worktree_repo_root
        {
            let cwd_owned = cwd.to_path_buf();
            let repo_root = repo_root.to_path_buf();
            let summary_for_rename = summary.clone();
            let rename_result = tokio::task::spawn_blocking(move || {
                let dir_name = cwd_owned.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let old_branch = format!("auto/{dir_name}");
                crate::auto_worktree::rename_auto_worktree_branch(
                    &repo_root,
                    &old_branch,
                    &summary_for_rename,
                )
            })
            .await;

            match rename_result {
                Ok(Ok(())) => {
                    debug!("Auto-worktree branch renamed based on summary");
                }
                Ok(Err(e)) => {
                    warn!("Failed to rename auto-worktree branch (non-fatal): {e}");
                }
                Err(e) => {
                    warn!("Auto-worktree branch rename task panicked (non-fatal): {e}");
                }
            }
        }

        let _ = event_tx
            .send(Event {
                id: String::new(),
                msg: EventMsg::PromptSummary(PromptSummaryEvent { summary }),
            })
            .await;
    }

    Ok(())
}

/// Return the custom commands directory: `{nori_home}/commands`.
pub(super) fn commands_dir(nori_home: &std::path::Path) -> PathBuf {
    nori_home.join("commands")
}

/// Execute session_start hooks and emit warnings for any failures.
/// Route parsed hook results to the appropriate event channels.
///
/// For each successful hook result with output:
/// - `Log` lines go to `tracing::info!`
/// - `Output`/`OutputWarn`/`OutputError` lines become `HookOutput` events
/// - `Context` lines accumulate into `pending_hook_context` (if provided)
///
/// Failed hooks emit `Warning` events.
pub(super) async fn route_hook_results(
    results: &[crate::hooks::HookResult],
    event_tx: &mpsc::Sender<Event>,
    event_id: &str,
    pending_hook_context: Option<&Mutex<Option<String>>>,
) {
    for result in results {
        if !result.success {
            if let Some(ref err) = result.error {
                let _ = event_tx
                    .send(Event {
                        id: event_id.to_string(),
                        msg: EventMsg::Warning(WarningEvent {
                            message: err.clone(),
                        }),
                    })
                    .await;
            }
            continue;
        }
        if let Some(ref output) = result.output {
            let parsed = crate::hooks::parse_hook_output(output);
            for line in parsed {
                match line {
                    crate::hooks::HookOutputLine::Log(msg) => {
                        tracing::info!("hook [{}]: {msg}", result.path);
                    }
                    crate::hooks::HookOutputLine::Output(msg) => {
                        let _ = event_tx
                            .send(Event {
                                id: event_id.to_string(),
                                msg: EventMsg::HookOutput(HookOutputEvent {
                                    message: msg,
                                    level: HookOutputLevel::Info,
                                }),
                            })
                            .await;
                    }
                    crate::hooks::HookOutputLine::OutputWarn(msg) => {
                        let _ = event_tx
                            .send(Event {
                                id: event_id.to_string(),
                                msg: EventMsg::HookOutput(HookOutputEvent {
                                    message: msg,
                                    level: HookOutputLevel::Warn,
                                }),
                            })
                            .await;
                    }
                    crate::hooks::HookOutputLine::OutputError(msg) => {
                        let _ = event_tx
                            .send(Event {
                                id: event_id.to_string(),
                                msg: EventMsg::HookOutput(HookOutputEvent {
                                    message: msg,
                                    level: HookOutputLevel::Error,
                                }),
                            })
                            .await;
                    }
                    crate::hooks::HookOutputLine::Context(ctx) => {
                        if let Some(lock) = pending_hook_context {
                            let mut guard = lock.lock().await;
                            match guard.as_mut() {
                                Some(existing) => {
                                    existing.push('\n');
                                    existing.push_str(&ctx);
                                }
                                None => {
                                    *guard = Some(ctx);
                                }
                            }
                        } else {
                            warn!(
                                "Hook emitted ::context:: line but this hook type does not support context injection; line discarded: {ctx}"
                            );
                        }
                    }
                }
            }
        }
    }
}

pub(super) async fn run_session_start_hooks(
    hooks: &[PathBuf],
    timeout: std::time::Duration,
    event_tx: &mpsc::Sender<Event>,
    pending_hook_context: Option<&Mutex<Option<String>>>,
) {
    if hooks.is_empty() {
        return;
    }
    let results = crate::hooks::execute_hooks(hooks, timeout).await;
    route_hook_results(&results, event_tx, "", pending_hook_context).await;
}

/// Generate a unique ID for operations
pub(super) fn generate_id() -> String {
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::Ordering;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!("acp-{}", COUNTER.fetch_add(1, Ordering::Relaxed))
}
