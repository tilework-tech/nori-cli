use serde_json::Value;
use tokio::sync::Mutex;

use super::*;
use crate::connection::local_mcp::LocalMcpServer;
use codex_protocol::protocol::ThreadGoalStatus;
use nori_protocol::ThreadGoalUpdated;
use sacp::Agent;
use sacp::ConnectTo;
use sacp::ConnectionTo;
use sacp::Dispatch;
use sacp::DynConnectTo;
use sacp::UntypedMessage;
use sacp::role;

const GET_GOAL_TOOL_NAME: &str = "get_goal";
const CREATE_GOAL_TOOL_NAME: &str = "create_goal";
const UPDATE_GOAL_TOOL_NAME: &str = "update_goal";

const DUPLICATE_CREATE_GOAL_ERROR: &str = "cannot create a new goal because this thread already has a goal; use update_goal only when the existing goal is complete";
const UPDATE_GOAL_STATUS_ERROR: &str = "update_goal can only mark the existing goal complete or blocked; pause, resume, budget-limited, and usage-limited status changes are controlled by the user or system";

#[derive(Clone)]
pub(crate) struct ThreadGoalMcpBridge {
    thread_goal_state: Arc<Mutex<thread_goal::ThreadGoalState>>,
    backend_event_tx: mpsc::Sender<BackendEvent>,
    transcript_recorder: Arc<Mutex<Option<Arc<TranscriptRecorder>>>>,
}

impl ThreadGoalMcpBridge {
    pub(crate) fn new(
        thread_goal_state: Arc<Mutex<thread_goal::ThreadGoalState>>,
        backend_event_tx: mpsc::Sender<BackendEvent>,
        transcript_recorder: Arc<Mutex<Option<Arc<TranscriptRecorder>>>>,
    ) -> Self {
        Self {
            thread_goal_state,
            backend_event_tx,
            transcript_recorder,
        }
    }

    pub(crate) async fn handle_mcp_request(&self, method: &str, params: Value) -> Value {
        match method {
            "initialize" => serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "nori-goal", "version": env!("CARGO_PKG_VERSION") }
            }),
            "tools/list" => serde_json::json!({ "tools": tools() }),
            "tools/call" => self.handle_tool_call(params).await,
            _ => tool_error(format!("unsupported goal MCP request: {method}")),
        }
    }

    async fn handle_tool_call(&self, params: Value) -> Value {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));

        match name {
            GET_GOAL_TOOL_NAME => self.get_goal().await,
            CREATE_GOAL_TOOL_NAME => self.create_goal(arguments).await,
            UPDATE_GOAL_TOOL_NAME => self.update_goal(arguments).await,
            "" => tool_error("tools/call requires a tool name"),
            other => tool_error(format!("unknown goal tool: {other}")),
        }
    }

    async fn get_goal(&self) -> Value {
        let state = self.thread_goal_state.lock().await;
        let goal = state.snapshot(thread_goal::now_seconds()).map(goal_json);
        tool_success(serde_json::json!({ "goal": goal }))
    }

    async fn create_goal(&self, arguments: Value) -> Value {
        if arguments.get("token_budget").is_some() {
            return tool_error("token budgets are not supported by Nori ACP goals yet");
        }

        let Some(objective) = arguments.get("objective").and_then(Value::as_str) else {
            return tool_error("create_goal requires objective");
        };
        let objective = objective.trim().to_string();
        let result = {
            let now = thread_goal::now_seconds();
            let mut state = self.thread_goal_state.lock().await;
            if state.snapshot(now).is_some() {
                return tool_error(DUPLICATE_CREATE_GOAL_ERROR);
            }
            state.set_objective(objective, Some(ThreadGoalStatus::Active), now)
        };

        match result {
            Ok(goal) => {
                self.emit_goal_updated(goal.clone()).await;
                tool_success(serde_json::json!({ "goal": goal_json(goal) }))
            }
            Err(err) => tool_error(err),
        }
    }

    async fn update_goal(&self, arguments: Value) -> Value {
        let Some(status) = arguments.get("status").and_then(Value::as_str) else {
            return tool_error("update_goal requires status");
        };
        let status = match status {
            "complete" => ThreadGoalStatus::Complete,
            "blocked" => ThreadGoalStatus::Blocked,
            "active" | "paused" | "usage_limited" | "budget_limited" => {
                return tool_error(UPDATE_GOAL_STATUS_ERROR);
            }
            other => return tool_error(format!("unsupported goal status: {other}")),
        };

        let result = {
            let now = thread_goal::now_seconds();
            let mut state = self.thread_goal_state.lock().await;
            state.set_status(status, now)
        };

        match result {
            Ok(goal) => {
                self.emit_goal_updated(goal.clone()).await;
                tool_success(serde_json::json!({ "goal": goal_json(goal) }))
            }
            Err(err) => tool_error(err),
        }
    }

    async fn emit_goal_updated(&self, goal: thread_goal::ThreadGoalSnapshot) {
        let recorder = self.transcript_recorder.lock().await.clone();
        emit_client_event(
            &self.backend_event_tx,
            recorder.as_ref(),
            ClientEvent::ThreadGoalUpdated(ThreadGoalUpdated {
                goal: goal.into_client_goal(),
            }),
        )
        .await;
    }
}

impl LocalMcpServer<Agent> for ThreadGoalMcpBridge {
    fn name(&self) -> String {
        "nori-goal".to_string()
    }

    fn connect(
        &self,
        _acp_url: String,
        _connection: ConnectionTo<Agent>,
    ) -> DynConnectTo<role::mcp::Client> {
        DynConnectTo::new(ThreadGoalMcpComponent {
            bridge: self.clone(),
        })
    }
}

struct ThreadGoalMcpComponent {
    bridge: ThreadGoalMcpBridge,
}

impl ConnectTo<role::mcp::Client> for ThreadGoalMcpComponent {
    async fn connect_to(
        self,
        client: impl ConnectTo<role::mcp::Server>,
    ) -> Result<(), sacp::Error> {
        let bridge = self.bridge;
        role::mcp::Server
            .builder()
            .on_receive_dispatch(
                async move |message: Dispatch, _connection| {
                    match message {
                        Dispatch::Request(request, responder) => {
                            let UntypedMessage { method, params } = request;
                            responder.respond(bridge.handle_mcp_request(&method, params).await)?;
                        }
                        Dispatch::Notification(_) | Dispatch::Response(_, _) => {}
                    }
                    Ok(())
                },
                sacp::on_receive_dispatch!(),
            )
            .connect_to(client)
            .await
    }
}

pub(super) fn register_for_session(
    connection: &SacpConnection,
    mcp_servers: &mut Vec<acp::McpServer>,
    thread_goal_state: Arc<Mutex<thread_goal::ThreadGoalState>>,
    backend_event_tx: mpsc::Sender<BackendEvent>,
    transcript_recorder: Arc<Mutex<Option<Arc<TranscriptRecorder>>>>,
) -> Result<()> {
    if !connection.capabilities().mcp_capabilities.http {
        return Ok(());
    }

    connection.register_local_mcp_server(
        mcp_servers,
        ThreadGoalMcpBridge::new(thread_goal_state, backend_event_tx, transcript_recorder),
    )
}

fn tools() -> Vec<Value> {
    vec![
        serde_json::json!({
            "name": GET_GOAL_TOOL_NAME,
            "description": "Get the current goal for this thread, including status, token and elapsed-time usage.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": CREATE_GOAL_TOOL_NAME,
            "description": format!(
                "Create a goal only when explicitly requested by the user or system/developer instructions; do not infer goals from ordinary tasks. Fails if a goal exists; use {UPDATE_GOAL_TOOL_NAME} only for status."
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "objective": {
                        "type": "string",
                        "description": "Required. The concrete objective to start pursuing. This starts a new active goal only when no goal is currently defined; if a goal already exists, this tool fails."
                    }
                },
                "required": ["objective"],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": UPDATE_GOAL_TOOL_NAME,
            "description": "Update the existing goal. Use this tool only to mark the goal achieved or genuinely blocked.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "enum": ["complete", "blocked"],
                        "description": "Required. Set to complete only when the objective is achieved and no required work remains. Set to blocked only after the same blocking condition has repeated and the agent is at an impasse."
                    }
                },
                "required": ["status"],
                "additionalProperties": false
            }
        }),
    ]
}

fn tool_success(body: Value) -> Value {
    tool_response(body, false)
}

fn tool_error(message: impl Into<String>) -> Value {
    tool_response(Value::String(message.into()), true)
}

fn tool_response(body: Value, is_error: bool) -> Value {
    let text = match body {
        Value::String(text) => text,
        other => serde_json::to_string(&other)
            .unwrap_or_else(|err| format!("failed to serialize goal MCP response: {err}")),
    };
    serde_json::json!({
        "content": [{ "type": "text", "text": text }],
        "isError": is_error
    })
}

fn goal_json(goal: thread_goal::ThreadGoalSnapshot) -> Value {
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

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::*;

    fn bridge() -> ThreadGoalMcpBridge {
        let (backend_event_tx, _backend_event_rx) = mpsc::channel(8);
        ThreadGoalMcpBridge::new(
            Arc::new(Mutex::new(thread_goal::ThreadGoalState::default())),
            backend_event_tx,
            Arc::new(Mutex::new(None)),
        )
    }

    fn tool_text(response: &Value) -> &str {
        response["content"][0]["text"]
            .as_str()
            .expect("tool response should contain text content")
    }

    fn is_error(response: &Value) -> bool {
        response["isError"].as_bool().unwrap_or(false)
    }

    fn parsed_tool_text(response: &Value) -> Value {
        serde_json::from_str(tool_text(response)).expect("tool text should be json")
    }

    fn tool_by_name<'a>(response: &'a Value, name: &str) -> &'a Value {
        response["tools"]
            .as_array()
            .expect("tools/list should return tools")
            .iter()
            .find(|tool| tool["name"] == name)
            .expect("expected tool to be listed")
    }

    #[tokio::test]
    async fn goal_mcp_lists_codex_compatible_goal_tools() {
        let response = bridge().handle_mcp_request("tools/list", json!({})).await;

        let get_goal = tool_by_name(&response, "get_goal");
        assert_eq!(
            get_goal["inputSchema"]["additionalProperties"],
            json!(false)
        );
        assert_eq!(
            tool_by_name(&response, "create_goal")["inputSchema"]["required"],
            json!(["objective"])
        );
        assert_eq!(
            tool_by_name(&response, "update_goal")["inputSchema"]["properties"]["status"]["enum"],
            json!(["complete", "blocked"])
        );
    }

    #[tokio::test]
    async fn get_goal_tool_returns_null_without_goal() {
        let response = bridge()
            .handle_mcp_request("tools/call", json!({ "name": "get_goal" }))
            .await;

        assert!(!is_error(&response));
        assert_eq!(parsed_tool_text(&response), json!({ "goal": null }));
    }

    #[tokio::test]
    async fn create_goal_tool_creates_active_goal_and_get_goal_reads_it() {
        let (backend_event_tx, mut backend_event_rx) = mpsc::channel(8);
        let bridge = ThreadGoalMcpBridge::new(
            Arc::new(Mutex::new(thread_goal::ThreadGoalState::default())),
            backend_event_tx,
            Arc::new(Mutex::new(None)),
        );

        let create_response = bridge
            .handle_mcp_request(
                "tools/call",
                json!({
                    "name": "create_goal",
                    "arguments": { "objective": "Ship the ACP goal bridge" }
                }),
            )
            .await;

        assert!(!is_error(&create_response));
        assert!(tool_text(&create_response).contains("Ship the ACP goal bridge"));
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

        let get_response = bridge
            .handle_mcp_request("tools/call", json!({ "name": "get_goal" }))
            .await;
        assert!(!is_error(&get_response));
        let goal = &parsed_tool_text(&get_response)["goal"];
        assert_eq!(goal["status"], "active");
        assert_eq!(goal["objective"], "Ship the ACP goal bridge");
    }

    #[tokio::test]
    async fn create_goal_tool_rejects_existing_goal() {
        let bridge = bridge();
        let first_response = bridge
            .handle_mcp_request(
                "tools/call",
                json!({
                    "name": "create_goal",
                    "arguments": { "objective": "First goal" }
                }),
            )
            .await;
        assert!(!is_error(&first_response));

        let second_response = bridge
            .handle_mcp_request(
                "tools/call",
                json!({
                    "name": "create_goal",
                    "arguments": { "objective": "Second goal" }
                }),
            )
            .await;

        assert!(is_error(&second_response));
        assert!(tool_text(&second_response).contains("already has a goal"));

        let get_response = bridge
            .handle_mcp_request("tools/call", json!({ "name": "get_goal" }))
            .await;
        assert_eq!(
            parsed_tool_text(&get_response)["goal"]["objective"],
            "First goal"
        );
    }

    #[tokio::test]
    async fn update_goal_tool_only_allows_complete_or_blocked() {
        let bridge = bridge();
        let create_response = bridge
            .handle_mcp_request(
                "tools/call",
                json!({
                    "name": "create_goal",
                    "arguments": { "objective": "Finish carefully" }
                }),
            )
            .await;
        assert!(!is_error(&create_response));

        let paused_response = bridge
            .handle_mcp_request(
                "tools/call",
                json!({
                    "name": "update_goal",
                    "arguments": { "status": "paused" }
                }),
            )
            .await;
        assert!(is_error(&paused_response));
        assert!(
            tool_text(&paused_response).contains("only mark the existing goal complete or blocked")
        );

        let get_response = bridge
            .handle_mcp_request("tools/call", json!({ "name": "get_goal" }))
            .await;
        assert_eq!(parsed_tool_text(&get_response)["goal"]["status"], "active");

        let blocked_response = bridge
            .handle_mcp_request(
                "tools/call",
                json!({
                    "name": "update_goal",
                    "arguments": { "status": "blocked" }
                }),
            )
            .await;
        assert!(!is_error(&blocked_response));
        assert_eq!(
            parsed_tool_text(&blocked_response)["goal"]["status"],
            "blocked"
        );
    }

    #[tokio::test]
    async fn update_goal_tool_marks_goal_complete() {
        let bridge = bridge();
        let create_response = bridge
            .handle_mcp_request(
                "tools/call",
                json!({
                    "name": "create_goal",
                    "arguments": { "objective": "Finish completely" }
                }),
            )
            .await;
        assert!(!is_error(&create_response));

        let complete_response = bridge
            .handle_mcp_request(
                "tools/call",
                json!({
                    "name": "update_goal",
                    "arguments": { "status": "complete" }
                }),
            )
            .await;
        assert!(!is_error(&complete_response));
        assert_eq!(
            parsed_tool_text(&complete_response)["goal"]["status"],
            "complete"
        );
    }

    #[tokio::test]
    async fn update_goal_tool_reports_missing_goal() {
        let response = bridge()
            .handle_mcp_request(
                "tools/call",
                json!({
                    "name": "update_goal",
                    "arguments": { "status": "complete" }
                }),
            )
            .await;

        assert!(is_error(&response));
        assert!(tool_text(&response).contains("no goal exists"));
    }
}
