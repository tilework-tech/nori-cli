use super::*;
use crate::follow_up::FollowUpIntent;
use crate::follow_up::FollowUpReceipt;
use crate::follow_up::FollowUpStatus;
use crate::follow_up::SENDER_META;
use crate::follow_up::Submission;
use crate::follow_up::steering_params;
use crate::follow_up::steering_result;
use crate::follow_up::supports_guarded_steering;
use sha2::Digest;
use sha2::Sha256;

/// A live session retains every admitted identity, including ambiguous deliveries.
/// Refuse fresh IDs at capacity so reconnects cannot turn eviction into a replay.
pub(super) type Receipts = HashMap<String, ([u8; 32], FollowUpReceipt)>;
const MAX_RECEIPTS: usize = 16_384;
const STEER_RECEIPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

impl AcpBackend {
    pub(crate) async fn submit_follow_up(
        &self,
        submission: Submission,
        response_tx: oneshot::Sender<Result<FollowUpReceipt>>,
    ) {
        let _ = self
            .session_event_tx
            .send(session_runtime_driver::SessionRuntimeInput::FollowUp {
                submission,
                response_tx,
            })
            .await;
    }

    pub(super) async fn deliver_follow_up(
        &self,
        submission: Submission,
    ) -> Result<FollowUpReceipt> {
        anyhow::ensure!(
            !submission.message_id.trim().is_empty() && submission.message_id.len() <= 256,
            "message_id must be 1–256 bytes"
        );
        anyhow::ensure!(
            !submission.sender.trim().is_empty() && submission.sender.len() <= 256,
            "sender must be 1–256 bytes"
        );
        anyhow::ensure!(
            submission.prompt.iter().any(|block| match block {
                acp::ContentBlock::Text(text) => !text.text.is_empty(),
                _ => true,
            }),
            "empty follow-up"
        );
        let digest: [u8; 32] = Sha256::digest(serde_json::to_vec(&submission)?).into();
        {
            let receipts = self.follow_ups.lock().await;
            if let Some((original, receipt)) = receipts.get(&submission.message_id) {
                anyhow::ensure!(original == &digest, "message_id identifies different input");
                return Ok(receipt.clone());
            }
            anyhow::ensure!(
                receipts.len() < MAX_RECEIPTS,
                "session follow-up receipt capacity reached"
            );
        }

        let sid = self.session_id.read().await.clone();
        let (phase, same_session, previous) = {
            let driver = self.session_driver.lock().await;
            (
                driver.public_phase(),
                driver.active_prompt_session.as_ref() == Some(&sid),
                driver.last_normal_turn.clone(),
            )
        };
        let active = same_session
            && matches!(&phase, nori_protocol::SessionPhase::Prompting { request_id }
            if request_id == &submission.expected_turn);
        let cancelling = same_session
            && matches!(&phase, nori_protocol::SessionPhase::Cancelling { request_id }
            if request_id == &submission.expected_turn);
        let ended = phase == nori_protocol::SessionPhase::Idle
            && previous.as_ref().is_some_and(|(original, owner)| {
                original == &sid && owner == &submission.expected_turn
            });
        let status = match submission.intent {
            _ if ended => FollowUpStatus::IdleQueued,
            FollowUpIntent::Queue if active || cancelling => FollowUpStatus::Queued,
            FollowUpIntent::Steer if active => {
                if supports_guarded_steering(self.connection.initialize_result()) {
                    self.steer(&sid, &submission).await?
                } else {
                    FollowUpStatus::UnsupportedQueued
                }
            }
            _ => FollowUpStatus::Rejected,
        };
        let receipt = submission.receipt(status);
        let meta = acp::Meta::from_iter([
            (
                nori_protocol::PROMPT_ECHO_ID_META_KEY.to_owned(),
                serde_json::json!(submission.message_id),
            ),
            (SENDER_META.to_owned(), serde_json::json!(submission.sender)),
        ]);
        match status {
            FollowUpStatus::Queued
            | FollowUpStatus::UnsupportedQueued
            | FollowUpStatus::IdleQueued => {
                let mut prompt = self
                    .prepare_user_input(
                        submission.prompt,
                        &submission.message_id,
                        Some(meta),
                        submission.origin,
                    )
                    .await;
                prompt.target_session = Some(sid);
                self.apply_session_event(session_reducer::InboundEvent::PromptSubmit(prompt))
                    .await;
            }
            FollowUpStatus::Injected => {
                let text = submission
                    .prompt
                    .iter()
                    .filter_map(|block| match block {
                        acp::ContentBlock::Text(text) => Some(text.text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                self.publish_user_input(
                    &submission.message_id,
                    Some(&text),
                    submission.prompt,
                    Some(&meta),
                )
                .await;
            }
            FollowUpStatus::Rejected | FollowUpStatus::Unknown => {}
        }
        self.follow_ups
            .lock()
            .await
            .insert(submission.message_id, (digest, receipt.clone()));
        Ok(receipt)
    }

    async fn steer(&self, sid: &acp::SessionId, submission: &Submission) -> Result<FollowUpStatus> {
        if self.session_driver.lock().await.steer_pending() {
            return Ok(FollowUpStatus::Rejected);
        }
        let params = steering_params(&sid.to_string(), &serde_json::to_value(&submission.prompt)?);
        let connection = Arc::clone(&self.connection);
        let mut request = tokio::spawn(async move {
            connection
                .send_ext_request("_session/steering", params)
                .await
        });

        let status = match tokio::time::timeout(STEER_RECEIPT_TIMEOUT, &mut request).await {
            Ok(Ok(Ok(result))) => steering_result(&result),
            Ok(Ok(Err(error))) => error
                .downcast_ref::<acp::Error>()
                .map(|error| crate::follow_up::steering_error(error.code.into()))
                .unwrap_or(FollowUpStatus::Unknown),
            Ok(Err(_)) => FollowUpStatus::Unknown,
            Err(_) => {
                self.session_driver.lock().await.wait_for_steer();
                let events = self.session_event_tx.clone();
                tokio::spawn(async move {
                    let _ = request.await;
                    let _ = events
                        .send(session_runtime_driver::SessionRuntimeInput::Reducer(
                            session_reducer::InboundEvent::SteerSettled,
                        ))
                        .await;
                });
                FollowUpStatus::Unknown
            }
        };
        Ok(status)
    }
}
