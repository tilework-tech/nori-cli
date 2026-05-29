//! The backend-owned `nori-client` MCP server.
//!
//! This is Nori's harness-side channel to the external ACP agent: the single
//! loopback MCP server through which Nori exposes harness-specific tooling that
//! the ACP protocol itself does not (yet) provide. Today it carries the goal
//! tools (`get_goal`/`create_goal`/`update_goal`); it is named `nori-client`
//! rather than `nori-goal` because it is the general point of contact, not a
//! goal-only surface.
//!
//! The transport is rmcp's spec-compliant streamable-HTTP server, served on a
//! loopback port. The tools are typed `#[tool]` handlers — there is no
//! hand-rolled HTTP or JSON-RPC framing here.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use anyhow::Context as _;
use anyhow::Result;
use rmcp::ErrorData as McpError;
use rmcp::RoleServer;
use rmcp::ServerHandler;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::model::Content;
use rmcp::model::Implementation;
use rmcp::model::InitializeRequestParam;
use rmcp::model::InitializeResult;
use rmcp::model::ProtocolVersion;
use rmcp::model::ServerCapabilities;
use rmcp::model::ServerInfo;
use rmcp::schemars;
use rmcp::service::RequestContext;
use rmcp::tool;
use rmcp::tool_handler;
use rmcp::tool_router;
use rmcp::transport::streamable_http_server::StreamableHttpService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::StreamableHttpServerConfig;
use serde::Deserialize;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use super::*;
use codex_protocol::protocol::ThreadGoalStatus;
use nori_protocol::ThreadGoalUpdated;

const DUPLICATE_CREATE_GOAL_ERROR: &str = "cannot create a new goal because this thread already has a goal; use update_goal only when the existing goal is complete";

#[derive(Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
struct CreateGoalRequest {
    /// Required. The concrete objective to start pursuing. This starts a new
    /// active goal only when no goal is currently defined; if a goal already
    /// exists, this tool fails.
    objective: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
struct UpdateGoalRequest {
    /// Required. Set to `complete` only when the objective is achieved and no
    /// required work remains. Set to `blocked` only after the same blocking
    /// condition has repeated and the agent is at an impasse.
    status: GoalCompletion,
}

/// The only goal transitions the agent may drive through `update_goal`. Other
/// statuses (active/paused/usage_limited/budget_limited) are owned by the user
/// or the backend, so they are intentionally not representable here.
#[derive(Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "lowercase")]
enum GoalCompletion {
    Complete,
    Blocked,
}

impl From<GoalCompletion> for ThreadGoalStatus {
    fn from(value: GoalCompletion) -> Self {
        match value {
            GoalCompletion::Complete => ThreadGoalStatus::Complete,
            GoalCompletion::Blocked => ThreadGoalStatus::Blocked,
        }
    }
}

/// The Arcs the `nori-client` tools operate on, shared across every request the
/// streamable-HTTP transport spawns. Cloned (cheaply) into each per-session
/// [`NoriClientService`] by the service factory.
#[derive(Clone)]
pub(crate) struct NoriClientShared {
    thread_goal_state: Arc<Mutex<thread_goal::ThreadGoalState>>,
    backend_event_tx: mpsc::Sender<BackendEvent>,
    transcript_recorder: Arc<Mutex<Option<Arc<TranscriptRecorder>>>>,
    connected: Arc<AtomicBool>,
}

impl NoriClientShared {
    pub(crate) fn new(
        thread_goal_state: Arc<Mutex<thread_goal::ThreadGoalState>>,
        backend_event_tx: mpsc::Sender<BackendEvent>,
        transcript_recorder: Arc<Mutex<Option<Arc<TranscriptRecorder>>>>,
        connected: Arc<AtomicBool>,
    ) -> Self {
        Self {
            thread_goal_state,
            backend_event_tx,
            transcript_recorder,
            connected,
        }
    }
}

/// One rmcp server handler instance. The transport constructs a fresh service
/// per session via the factory; all instances share the same [`NoriClientShared`]
/// Arcs, so goal state and the connected flag are global to the backend.
#[derive(Clone)]
pub(crate) struct NoriClientService {
    shared: NoriClientShared,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl NoriClientService {
    fn new(shared: NoriClientShared) -> Self {
        Self {
            shared,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Get the current goal for this thread, including status, token and elapsed-time usage."
    )]
    async fn get_goal(&self) -> Result<CallToolResult, McpError> {
        let state = self.shared.thread_goal_state.lock().await;
        let goal = state.snapshot(thread_goal::now_seconds()).map(goal_json);
        Ok(tool_success(serde_json::json!({ "goal": goal })))
    }

    #[tool(
        description = "Create a goal only when explicitly requested by the user or system/developer instructions; do not infer goals from ordinary tasks. Fails if a goal exists; use update_goal only for status."
    )]
    async fn create_goal(
        &self,
        Parameters(CreateGoalRequest { objective }): Parameters<CreateGoalRequest>,
    ) -> Result<CallToolResult, McpError> {
        let objective = objective.trim().to_string();
        let result = {
            let now = thread_goal::now_seconds();
            let mut state = self.shared.thread_goal_state.lock().await;
            if state.snapshot(now).is_some() {
                return Ok(tool_error(DUPLICATE_CREATE_GOAL_ERROR));
            }
            state.set_objective(objective, Some(ThreadGoalStatus::Active), now)
        };
        Ok(self.respond_with_goal(result).await)
    }

    #[tool(
        description = "Update the existing goal. Use this tool only to mark the goal achieved or genuinely blocked. Pause, resume, budget-limited, and usage-limited transitions are controlled by the user or system, not this tool."
    )]
    async fn update_goal(
        &self,
        Parameters(UpdateGoalRequest { status }): Parameters<UpdateGoalRequest>,
    ) -> Result<CallToolResult, McpError> {
        let result = {
            let now = thread_goal::now_seconds();
            let mut state = self.shared.thread_goal_state.lock().await;
            state.set_status(status.into(), now)
        };
        Ok(self.respond_with_goal(result).await)
    }

    /// Emit a `ThreadGoalUpdated` client event and render the tool reply for a
    /// state mutation, or surface the validation error as a tool error.
    async fn respond_with_goal(
        &self,
        result: Result<thread_goal::ThreadGoalSnapshot, String>,
    ) -> CallToolResult {
        match result {
            Ok(goal) => {
                let recorder = self.shared.transcript_recorder.lock().await.clone();
                emit_client_event(
                    &self.shared.backend_event_tx,
                    recorder.as_ref(),
                    ClientEvent::ThreadGoalUpdated(ThreadGoalUpdated {
                        goal: goal.clone().into_client_goal(),
                    }),
                )
                .await;
                tool_success(serde_json::json!({ "goal": goal_json(goal) }))
            }
            Err(err) => tool_error(err),
        }
    }
}

#[tool_handler]
impl ServerHandler for NoriClientService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "nori-client".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                ..Default::default()
            },
            instructions: None,
        }
    }

    /// Mark the agent connected on initialize. This flag is the sole gate that
    /// lets hidden goal continuations chain (see the safety invariant in
    /// `session_runtime_driver.rs`): an agent that never opens this server can
    /// never reach `update_goal`, so it must not be allowed to self-chain.
    async fn initialize(
        &self,
        request: InitializeRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        self.shared.connected.store(true, Ordering::Relaxed);
        if context.peer.peer_info().is_none() {
            context.peer.set_peer_info(request);
        }
        Ok(self.get_info())
    }
}

fn tool_success(body: serde_json::Value) -> CallToolResult {
    CallToolResult::success(vec![Content::text(body.to_string())])
}

fn tool_error(message: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![Content::text(message.into())])
}

fn goal_json(goal: thread_goal::ThreadGoalSnapshot) -> serde_json::Value {
    serde_json::json!({
        "objective": goal.objective,
        "status": status_label(goal.status),
        "tokens_used": goal.tokens_used,
        "token_budget": null,
        "tokens_remaining": null,
        "time_used_seconds": goal.time_used_seconds,
        "created_at": goal.created_at,
        "updated_at": goal.updated_at,
    })
}

fn status_label(status: ThreadGoalStatus) -> &'static str {
    match status {
        ThreadGoalStatus::Active => "active",
        ThreadGoalStatus::Paused => "paused",
        ThreadGoalStatus::Blocked => "blocked",
        ThreadGoalStatus::UsageLimited => "usage_limited",
        ThreadGoalStatus::BudgetLimited => "budget_limited",
        ThreadGoalStatus::Complete => "complete",
    }
}

/// Loopback `nori-client` MCP server. Bound on first use and torn down on drop.
pub(crate) struct NoriClientServer {
    url: String,
    abort_handle: tokio::task::AbortHandle,
}

impl NoriClientServer {
    pub(crate) async fn spawn(shared: NoriClientShared) -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .context("failed to bind local nori-client MCP server")?;
        let url = format!("http://{}/mcp", listener.local_addr()?);

        let service = StreamableHttpService::new(
            move || Ok(NoriClientService::new(shared.clone())),
            Arc::new(LocalSessionManager::default()),
            StreamableHttpServerConfig {
                stateful_mode: false,
                sse_keep_alive: None,
                ..Default::default()
            },
        );
        let router = axum::Router::new().nest_service("/mcp", service);
        let task = tokio::spawn(async move {
            if let Err(err) = axum::serve(listener, router).await {
                tracing::debug!("nori-client MCP server stopped: {err}");
            }
        });

        Ok(Self {
            url,
            abort_handle: task.abort_handle(),
        })
    }

    pub(crate) fn url(&self) -> &str {
        &self.url
    }
}

impl Drop for NoriClientServer {
    fn drop(&mut self) {
        self.abort_handle.abort();
    }
}

pub(super) async fn register_for_session(
    connection: &SacpConnection,
    mcp_servers: &mut Vec<acp::McpServer>,
    thread_goal_state: Arc<Mutex<thread_goal::ThreadGoalState>>,
    backend_event_tx: mpsc::Sender<BackendEvent>,
    transcript_recorder: Arc<Mutex<Option<Arc<TranscriptRecorder>>>>,
    connected: Arc<AtomicBool>,
    http_server: Arc<Mutex<Option<NoriClientServer>>>,
) -> Result<()> {
    connected.store(false, Ordering::Relaxed);

    if !connection.capabilities().mcp_capabilities.http {
        return Ok(());
    }

    let mut server = http_server.lock().await;
    if server.is_none() {
        *server = Some(
            NoriClientServer::spawn(NoriClientShared::new(
                thread_goal_state,
                backend_event_tx,
                transcript_recorder,
                Arc::clone(&connected),
            ))
            .await?,
        );
    }
    let Some(server) = server.as_ref() else {
        return Ok(());
    };
    mcp_servers.push(acp::McpServer::Http(acp::McpServerHttp::new(
        "nori-client",
        server.url().to_string(),
    )));

    Ok(())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::*;

    fn service() -> NoriClientService {
        let (backend_event_tx, _backend_event_rx) = mpsc::channel(8);
        NoriClientService::new(NoriClientShared::new(
            Arc::new(Mutex::new(thread_goal::ThreadGoalState::default())),
            backend_event_tx,
            Arc::new(Mutex::new(None)),
            Arc::new(AtomicBool::new(false)),
        ))
    }

    fn tool_text(result: &CallToolResult) -> &str {
        result.content[0]
            .as_text()
            .expect("tool result should contain text content")
            .text
            .as_str()
    }

    fn is_error(result: &CallToolResult) -> bool {
        result.is_error.unwrap_or(false)
    }

    fn parsed_tool_text(result: &CallToolResult) -> serde_json::Value {
        serde_json::from_str(tool_text(result)).expect("tool text should be json")
    }

    async fn create(service: &NoriClientService, objective: &str) -> CallToolResult {
        service
            .create_goal(Parameters(CreateGoalRequest {
                objective: objective.to_string(),
            }))
            .await
            .expect("create_goal should not fail at the protocol level")
    }

    async fn get(service: &NoriClientService) -> CallToolResult {
        service
            .get_goal()
            .await
            .expect("get_goal should not fail at the protocol level")
    }

    async fn update(service: &NoriClientService, status: GoalCompletion) -> CallToolResult {
        service
            .update_goal(Parameters(UpdateGoalRequest { status }))
            .await
            .expect("update_goal should not fail at the protocol level")
    }

    #[test]
    fn get_info_advertises_nori_client_with_tools() {
        let info = service().get_info();
        assert_eq!(info.server_info.name, "nori-client");
        assert!(info.capabilities.tools.is_some());
    }

    #[tokio::test]
    async fn get_goal_returns_null_without_goal() {
        let result = get(&service()).await;
        assert!(!is_error(&result));
        assert_eq!(parsed_tool_text(&result), json!({ "goal": null }));
    }

    #[tokio::test]
    async fn create_goal_creates_active_goal_and_emits_event() {
        let (backend_event_tx, mut backend_event_rx) = mpsc::channel(8);
        let service = NoriClientService::new(NoriClientShared::new(
            Arc::new(Mutex::new(thread_goal::ThreadGoalState::default())),
            backend_event_tx,
            Arc::new(Mutex::new(None)),
            Arc::new(AtomicBool::new(false)),
        ));

        let create_result = create(&service, "Ship the ACP goal bridge").await;
        assert!(!is_error(&create_result));
        assert!(tool_text(&create_result).contains("Ship the ACP goal bridge"));

        let emitted_event = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            backend_event_rx.recv(),
        )
        .await
        .expect("create_goal should emit a client event before timeout")
        .expect("create_goal should emit a client event");
        match emitted_event {
            BackendEvent::Client(ClientEvent::ThreadGoalUpdated(update)) => {
                assert_eq!(update.goal.objective, "Ship the ACP goal bridge");
            }
            other => panic!("expected thread goal update, got {other:?}"),
        }

        let get_result = get(&service).await;
        let goal = &parsed_tool_text(&get_result)["goal"];
        assert_eq!(goal["status"], "active");
        assert_eq!(goal["objective"], "Ship the ACP goal bridge");
    }

    #[tokio::test]
    async fn create_goal_rejects_existing_goal() {
        let service = service();
        assert!(!is_error(&create(&service, "First goal").await));

        let second = create(&service, "Second goal").await;
        assert!(is_error(&second));
        assert!(tool_text(&second).contains("already has a goal"));

        let goal = parsed_tool_text(&get(&service).await);
        assert_eq!(goal["goal"]["objective"], "First goal");
    }

    #[tokio::test]
    async fn update_goal_marks_goal_complete() {
        let service = service();
        assert!(!is_error(&create(&service, "Finish completely").await));

        let complete = update(&service, GoalCompletion::Complete).await;
        assert!(!is_error(&complete));
        assert_eq!(parsed_tool_text(&complete)["goal"]["status"], "complete");
    }

    #[tokio::test]
    async fn update_goal_marks_goal_blocked() {
        let service = service();
        assert!(!is_error(&create(&service, "Finish carefully").await));

        let blocked = update(&service, GoalCompletion::Blocked).await;
        assert!(!is_error(&blocked));
        assert_eq!(parsed_tool_text(&blocked)["goal"]["status"], "blocked");
    }

    #[tokio::test]
    async fn update_goal_reports_missing_goal() {
        let result = update(&service(), GoalCompletion::Complete).await;
        assert!(is_error(&result));
        assert!(tool_text(&result).contains("no goal exists"));
    }
}
