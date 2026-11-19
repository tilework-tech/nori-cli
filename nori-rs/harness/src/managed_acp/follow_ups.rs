//! Follow-up admission and ordering within a single admitted ACP generation.

use super::*;
use crate::follow_up::FollowUpIntent;
use crate::follow_up::FollowUpReceipt;
use crate::follow_up::FollowUpStatus;
use crate::follow_up::SENDER_META;
use crate::follow_up::steering_error;
use crate::follow_up::steering_params;
use crate::follow_up::steering_result;
use crate::follow_up::supports_guarded_steering;
use crate::follow_up_rpc::FollowUpParams;
use std::collections::VecDeque;

pub(super) const SUBMITTED: &str = "_nori/session/follow_up/submitted";
pub(super) const STARTED: &str = "_nori/session/follow_up/started";
pub(super) const FINISHED: &str = "_nori/session/follow_up/finished";
const SENDER: &str = "machine-client";
const RECEIPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const MAX_MESSAGES: usize = 4096;
const MAX_TURN_IDS: usize = 65536;

struct Message {
    params: FollowUpParams,
    // The digest remains after delivery so retries cannot change the content.
    digest: [u8; 32],
    status: FollowUpStatus,
    reply_to: Option<Value>,
}

impl Message {
    fn receipt(&self) -> FollowUpReceipt {
        FollowUpReceipt {
            message_id: self.params.message_id.clone(),
            sender: SENDER.into(),
            status: self.status,
        }
    }
}

#[derive(Default)]
struct Turn {
    owner: Value,
    // A reused JSON-RPC id cannot identify a particular incarnation of a turn.
    ids: HashSet<Value>,
    ambiguous: bool,
    active: bool,
    cancelling: bool,
    can_drain: bool,
    permissions: HashSet<Value>,
    steer: Option<(Value, String)>,
    deadline: Option<tokio::time::Instant>,
    closing: bool,
    queue: VecDeque<String>,
    queued_owner: Option<String>,
}

#[derive(Default)]
pub(super) struct FollowUps {
    turns: HashMap<String, Turn>,
    messages: HashMap<(String, String), Message>,
}

impl FollowUps {
    pub(super) fn deadline(&self) -> Option<tokio::time::Instant> {
        self.turns.values().filter_map(|t| t.deadline).min()
    }

    pub(super) fn closing(&mut self, sid: &str, closing: bool) {
        if let Some(turn) = self.turns.get_mut(sid) {
            turn.closing = closing;
        }
    }

    pub(super) fn cancel(&mut self, sid: &str) {
        if let Some(turn) = self.turns.get_mut(sid) {
            turn.cancelling = true;
        }
    }

    pub(super) fn busy(&self, sid: &str) -> bool {
        self.turns.get(sid).is_some_and(|turn| {
            turn.steer.is_some()
                || (!turn.queue.is_empty() && (turn.can_drain || !turn.permissions.is_empty()))
        })
    }

    pub(super) fn reserved(&self, id: &Value) -> bool {
        self.turns
            .values()
            .any(|t| t.steer.as_ref().is_some_and(|(wire, _)| wire == id))
            || self
                .messages
                .values()
                .any(|m| m.reply_to.as_ref() == Some(id))
    }

    pub(super) fn start(&mut self, sid: &str, id: &Value) {
        let turn = self.turns.entry(sid.to_owned()).or_default();
        turn.ambiguous = turn.ids.len() >= MAX_TURN_IDS || !turn.ids.insert(id.clone());
        turn.owner = id.clone();
        turn.active = true;
        turn.cancelling = false;
        turn.can_drain = false;
    }

    pub(super) fn permission(&mut self, sid: &str, id: &Value) {
        if let Some(turn) = self.turns.get_mut(sid) {
            turn.permissions.insert(id.clone());
        }
    }

    pub(super) fn answer(&mut self, id: &Value) -> Option<String> {
        self.turns
            .iter_mut()
            .find_map(|(sid, turn)| turn.permissions.remove(id).then(|| sid.clone()))
    }
}

fn error(id: &Value, code: i64, message: impl std::fmt::Display) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message.to_string()}})
}

impl Relay {
    pub(super) async fn submit_follow_up(
        &mut self,
        id: &Value,
        value: Value,
        writer: &mpsc::Sender<Value>,
    ) -> Result<()> {
        use sha2::Digest;
        use sha2::Sha256;
        let params = match FollowUpParams::parse(value) {
            Ok(params) => params,
            Err(err) => return Ok(self.output.send(error(id, -32602, err)).await?),
        };
        let sid = params.session_id.clone();
        let key = (sid.clone(), params.message_id.clone());
        let digest: [u8; 32] = Sha256::digest(serde_json::to_vec(&params)?).into();
        if let Some(previous) = self.follow_ups.messages.get(&key) {
            let reply = if previous.digest == digest {
                json!({"jsonrpc":"2.0","id":id,"result":previous.receipt()})
            } else {
                error(id, -32602, "message_id was used for different input")
            };
            return Ok(self.output.send(reply).await?);
        }
        if self.follow_ups.messages.len() >= MAX_MESSAGES {
            return Ok(self
                .output
                .send(error(id, -32000, "follow-up receipt capacity reached"))
                .await?);
        }
        let turn = self.follow_ups.turns.get_mut(&sid);
        let expected = json!(params.expected_turn);
        let status = match turn {
            Some(t) if t.cancelling && t.active && params.intent == FollowUpIntent::Steer => {
                FollowUpStatus::Rejected
            }
            Some(t)
                if t.active
                    && t.steer.is_some()
                    && t.deadline.is_none()
                    && params.intent == FollowUpIntent::Steer =>
            {
                FollowUpStatus::Rejected
            }
            Some(t)
                if !t.closing
                    && !t.ambiguous
                    && t.owner == expected
                    && (t.active || t.can_drain) =>
            {
                t.queue.push_back(params.message_id.clone());
                if !t.active {
                    FollowUpStatus::IdleQueued
                } else if params.intent == FollowUpIntent::Queue {
                    FollowUpStatus::Queued
                } else if !self
                    .initialize
                    .as_ref()
                    .is_some_and(supports_guarded_steering)
                {
                    FollowUpStatus::UnsupportedQueued
                } else {
                    FollowUpStatus::Unknown
                }
            }
            _ => FollowUpStatus::Rejected,
        };
        let mut msg = Message {
            params,
            digest,
            status,
            reply_to: (status == FollowUpStatus::Unknown).then(|| id.clone()),
        };
        if msg.reply_to.is_none() {
            self.output
                .send(json!({"jsonrpc":"2.0","id":id,"result":msg.receipt()}))
                .await?;
        }
        if status == FollowUpStatus::Rejected {
            msg.params.prompt.clear();
        }
        self.follow_ups.messages.insert(key, msg);
        self.advance_follow_ups(&sid, writer).await
    }

    pub(super) async fn advance_follow_ups(
        &mut self,
        sid: &str,
        writer: &mpsc::Sender<Value>,
    ) -> Result<()> {
        let Some(turn) = self.follow_ups.turns.get_mut(sid) else {
            return Ok(());
        };
        if turn.steer.is_some() || turn.closing {
            return Ok(());
        }
        let mut refused = Vec::new();
        for key in &turn.queue {
            let msg = self
                .follow_ups
                .messages
                .get_mut(&(sid.to_owned(), key.clone()))
                .context("missing queued follow-up")?;
            if msg.reply_to.is_none() {
                continue;
            }
            if turn.active && turn.cancelling {
                msg.status = FollowUpStatus::Rejected;
                self.output
                    .send(json!({"jsonrpc":"2.0","id":msg.reply_to.take(),"result":msg.receipt()}))
                    .await?;
                msg.params.prompt.clear();
                refused.push(key.clone());
                continue;
            }
            if turn.active {
                let id = json!(uuid::Uuid::new_v4().to_string());
                if self.echo {
                    self.output.send(json!({"jsonrpc":"2.0","method":SUBMITTED,"params":{
                        "sessionId":sid,"turn_id":turn.owner,"message_id":key,"sender":SENDER,"prompt":msg.params.prompt
                    }})).await?;
                }
                self.native.write(
                    writer,
                    json!({"jsonrpc":"2.0","method":"_session/steering","id":id,
                    "params":steering_params(sid,&json!(msg.params.prompt))}),
                )?;
                turn.steer = Some((id, key.clone()));
                turn.deadline = Some(tokio::time::Instant::now() + RECEIPT_TIMEOUT);
                return Ok(());
            }
            // The guarded steer was still waiting its turn to write when its owner ended.
            msg.status = FollowUpStatus::IdleQueued;
            self.output
                .send(json!({"jsonrpc":"2.0","id":msg.reply_to.take(),"result":msg.receipt()}))
                .await?;
        }
        turn.queue.retain(|key| !refused.contains(key));
        if turn.active || !turn.can_drain || !turn.permissions.is_empty() {
            return Ok(());
        }
        let Some(key) = turn.queue.pop_front() else {
            return Ok(());
        };
        let msg = self
            .follow_ups
            .messages
            .get_mut(&(sid.to_owned(), key.clone()))
            .context("missing queued follow-up")?;
        let wire = json!(format!("nori-follow-up-{}", uuid::Uuid::new_v4()));
        let params = json!({"sessionId":sid,"prompt":std::mem::take(&mut msg.params.prompt),"_meta":{
            (nori_protocol::PROMPT_ECHO_ID_META_KEY):key,(SENDER_META):SENDER
        }});
        turn.queued_owner = Some(key.clone());
        self.output.send(json!({"jsonrpc":"2.0","method":STARTED,"params":{"sessionId":sid,"message_id":key,"sender":SENDER,"turn_id":wire}})).await?;
        self.send_prompt(sid, params, wire, writer).await
    }

    pub(super) async fn steer_reply(
        &mut self,
        message: &Value,
        writer: &mpsc::Sender<Value>,
    ) -> Result<bool> {
        let Some((sid, turn)) = self
            .follow_ups
            .turns
            .iter_mut()
            .find(|(_, t)| t.steer.as_ref().is_some_and(|(id, _)| id == &message["id"]))
        else {
            return Ok(false);
        };
        turn.deadline = None;
        let (_, key) = turn.steer.take().context("missing steer request")?;
        let sid = sid.clone();
        let msg = self
            .follow_ups
            .messages
            .get_mut(&(sid.clone(), key.clone()))
            .context("missing steer message")?;
        msg.status = if msg.reply_to.is_none() {
            // A caller already received an unknown receipt. A late reply releases
            // the wire fence, but cannot schedule another delivery.
            FollowUpStatus::Unknown
        } else if let Some(code) = message["error"]["code"]
            .as_i64()
            .and_then(|code| i32::try_from(code).ok())
        {
            steering_error(code)
        } else {
            steering_result(&message["result"])
        };
        if !matches!(
            msg.status,
            FollowUpStatus::IdleQueued | FollowUpStatus::UnsupportedQueued
        ) {
            turn.queue.retain(|queued| queued != &key);
        }
        if let Some(id) = msg.reply_to.take() {
            self.output
                .send(json!({"jsonrpc":"2.0","id":id,"result":msg.receipt()}))
                .await?;
        }
        let echo = (msg.status == FollowUpStatus::Injected).then(|| json!({
            "prompt":msg.params.prompt,"_meta":{(nori_protocol::PROMPT_ECHO_ID_META_KEY):key,(SENDER_META):SENDER}
        }));
        if !matches!(
            msg.status,
            FollowUpStatus::IdleQueued | FollowUpStatus::UnsupportedQueued
        ) {
            msg.params.prompt.clear();
        }
        if let Some(mut params) = echo {
            self.record_input(&sid, &mut params, Some(SENDER)).await?;
        }
        self.advance_follow_ups(&sid, writer).await?;
        Ok(true)
    }

    pub(super) async fn expire_steers(&mut self) -> Result<()> {
        let now = tokio::time::Instant::now();
        for (sid, turn) in &mut self.follow_ups.turns {
            if turn.deadline.is_some_and(|at| at <= now) {
                turn.deadline = None;
                let mut retained = VecDeque::new();
                for key in turn.queue.drain(..) {
                    let msg = self
                        .follow_ups
                        .messages
                        .get_mut(&(sid.clone(), key.clone()))
                        .context("missing queued follow-up")?;
                    let inflight = turn.steer.as_ref().is_some_and(|(_, k)| k == &key);
                    if let Some(id) = msg.reply_to.take() {
                        msg.status = if inflight {
                            FollowUpStatus::Unknown
                        } else {
                            FollowUpStatus::Rejected
                        };
                        self.output
                            .send(json!({"jsonrpc":"2.0","id":id,"result":msg.receipt()}))
                            .await?;
                        if !inflight {
                            msg.params.prompt.clear();
                            continue;
                        }
                    }
                    retained.push_back(key);
                }
                turn.queue = retained;
            }
        }
        Ok(())
    }

    pub(super) async fn closed_follow_ups(&mut self, sid: &str) -> Result<()> {
        let Some(turn) = self.follow_ups.turns.get_mut(sid) else {
            return Ok(());
        };
        turn.closing = true;
        turn.active = false;
        turn.permissions.clear();
        turn.can_drain = false;
        for key in turn.queue.drain(..) {
            let msg = self
                .follow_ups
                .messages
                .get_mut(&(sid.to_owned(), key.clone()))
                .context("missing queued follow-up")?;
            if let Some(id) = msg.reply_to.take() {
                msg.status = if turn.steer.as_ref().is_some_and(|(_, k)| k == &key) {
                    FollowUpStatus::Unknown
                } else {
                    FollowUpStatus::Rejected
                };
                self.output
                    .send(json!({"jsonrpc":"2.0","id":id,"result":msg.receipt()}))
                    .await?;
            } else {
                self.output.send(json!({"jsonrpc":"2.0","method":FINISHED,"params":{"sessionId":sid,"message_id":key,"sender":SENDER,"error":{"code":-32002,"message":"session closed before queue dispatch"}}})).await?;
            }
            msg.params.prompt.clear();
        }
        Ok(())
    }

    pub(super) async fn terminal(
        &mut self,
        sid: &str,
        message: Value,
        writer: &mpsc::Sender<Value>,
    ) -> Result<()> {
        let turn = self
            .follow_ups
            .turns
            .get_mut(sid)
            .context("missing prompt owner")?;
        turn.can_drain = turn.active
            && !turn.closing
            && turn.owner == message["id"]
            && message["result"]["stopReason"] == "end_turn";
        turn.active = false;
        if let Some(key) = turn.queued_owner.take() {
            self.output.send(json!({"jsonrpc":"2.0","method":FINISHED,"params":{"sessionId":sid,"message_id":key,
                "sender":SENDER,"turn_id":message["id"],"result":message.get("result"),"error":message.get("error")}})).await?;
        } else {
            self.output.send(message).await?;
        }
        self.advance_follow_ups(sid, writer).await
    }
}
