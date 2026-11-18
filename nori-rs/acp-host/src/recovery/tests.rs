use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn only_the_primary_session_reaches_replay_and_load_history_is_excluded() -> Result<()> {
    let root = tempfile::tempdir()?;
    let mut host = Recovery::open(root.path())?;
    let scope = Scope {
        binding_id: "fixture".into(),
        generation: 2,
    };
    host.journal
        .register_identity("fixture", 2, "conversation", None)?;
    host.attach(&scope);
    for (id, session) in [(1, "primary"), (2, "side")] {
        host.incoming(
            &scope,
            &json!({"jsonrpc":"2.0","id":id,"method":"session/new","params":{}}),
            None,
        )?;
        host.outgoing(&scope, &json!({"id":id,"result":{"sessionId":session}}))?;
    }
    let event = |session: &str| json!({"method":"session/update","params":{"sessionId":session,"update":{"sessionUpdate":"agent_message_chunk","opaque":43}}});
    assert!(host.outgoing(&scope, &event("side"))?.is_none());
    let source = host
        .outgoing(&scope, &event("primary"))?
        .context("missing primary source")?;
    host.incoming(
        &scope,
        &json!({"jsonrpc":"2.0","id":3,"method":"session/load","params":{"sessionId":"primary"}}),
        None,
    )?;
    assert!(host.outgoing(&scope, &event("primary"))?.is_none());
    assert!(host.checkpoint(&scope).is_err());
    host.outgoing(&scope, &json!({"id":3,"result":{}}))?;
    let records = host.journal.replay("fixture", 2)?;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].source, source);
    assert_eq!(records[0].message, event("primary"));
    Ok(())
}

#[tokio::test]
async fn release_waits_for_a_registered_producer_and_receives_its_failure() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let recovery = SharedRecovery::open(dir.path())?;
    let scope = Scope {
        binding_id: "fixture".into(),
        generation: 1,
    };
    let chosen = scope.clone();
    recovery
        .run(move |r| {
            r.journal.register(&chosen.binding_id, chosen.generation)?;
            r.attach(&chosen);
            Ok(())
        })
        .await?;
    recovery.wait_for_producer(scope.clone()).await?;
    let producer = recovery.start_producer(scope.clone()).await?;
    let waiter = recovery.wait_for_producer(scope.clone());
    tokio::pin!(waiter);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(30), &mut waiter)
            .await
            .is_err()
    );
    producer
        .finish(Some("fixture writer failure".to_owned()))
        .await?;
    assert!(waiter.await.is_err());
    Ok(())
}

#[test]
fn invalid_request_id_is_rejected_without_wedging_the_checkpoint() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut recovery = Recovery::open(dir.path())?;
    let scope = Scope {
        binding_id: "fixture".into(),
        generation: 1,
    };
    recovery.journal.register("fixture", 1)?;
    recovery.attach(&scope);
    let malformed = json!({"jsonrpc":"2.0","id":{},"method":"session/new","params":{}});
    assert!(recovery.incoming(&scope, &malformed, None).is_err());
    recovery.checkpoint(&scope)?;
    Ok(())
}

#[tokio::test]
async fn abandoned_producer_prevents_a_successful_checkpoint() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let recovery = SharedRecovery::open(dir.path())?;
    let scope = Scope {
        binding_id: "fixture".into(),
        generation: 1,
    };
    let selected = scope.clone();
    recovery
        .run(move |r| {
            r.journal
                .register(&selected.binding_id, selected.generation)?;
            r.attach(&selected);
            Ok(())
        })
        .await?;
    drop(recovery.start_producer(scope.clone()).await?);
    assert!(recovery.wait_for_producer(scope.clone()).await.is_err());
    assert!(recovery.run(move |r| r.checkpoint(&scope)).await.is_err());
    Ok(())
}
