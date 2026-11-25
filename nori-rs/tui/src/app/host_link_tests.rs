use std::sync::Arc;
use std::time::Duration;

use nori_harness::remote_agent::ManagedAgent;
use nori_harness::remote_agent::ManagedClient;
use nori_harness::remote_agent::ManagedReply;
use nori_harness::remote_agent::RemoteAcpServer;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::watch;

use super::host_link::HostLink;
use super::host_link::HostUpdate;
use crate::app_event_sender::HostReply;
use crate::host_client::HostCall;
use crate::host_client::HostConnection;
use crate::host_client::HostEndpoint;
use crate::host_client::HostFeatures;
use nori_protocol::ControlRole;

const TOKEN: &str = "synthetic-token";

/// A host whose sessions answer everything at once, except a file search,
/// which it receives and never answers.
#[derive(Default)]
struct QuietHost {
    searching: tokio::sync::Notify,
}
impl ManagedAgent for QuietHost {
    async fn request(&self, request: Value) -> ManagedReply {
        if request["method"] == "_nori/session/find_paths" {
            self.searching.notify_one();
            std::future::pending::<()>().await;
        }
        ManagedReply {
            delivery_guard: None,
            response: json!({"jsonrpc":"2.0", "id":request["id"], "result":{}}),
            subscription: None,
        }
    }
}

async fn host() -> RemoteAcpServer {
    host_with(Arc::default()).await
}

async fn host_with(agent: Arc<QuietHost>) -> RemoteAcpServer {
    RemoteAcpServer::bind_host("127.0.0.1:0".parse().unwrap(), agent, TOKEN.into())
        .await
        .unwrap()
}

const WAIT: Duration = Duration::from_secs(20);

async fn settled<T>(receipt: tokio::sync::oneshot::Receiver<T>) -> T {
    tokio::time::timeout(WAIT, receipt)
        .await
        .expect("the call must be settled")
        .expect("the reply must be delivered")
}

async fn join(endpoint: String) -> HostLink {
    let initial = ManagedClient::connect(&endpoint, TOKEN).await.unwrap();
    HostLink::join(
        HostEndpoint {
            endpoint,
            token: TOKEN.into(),
            local_token: None,
            refresh_local_token: None,
        },
        initial,
    )
}

fn endpoint_of(addr: std::net::SocketAddr) -> String {
    format!("ws://{addr}/sessions")
}

/// The next update the App would act on, skipping traffic it would not see.
async fn next_update(link: &mut HostLink) -> HostUpdate {
    tokio::time::timeout(WAIT, async {
        loop {
            let event = link.next().await.expect("the link must keep running");
            if let Some(update) = link.handle(event).expect("a joined link never fails") {
                return update;
            }
        }
    })
    .await
    .expect("an update must arrive")
}

/// Waits until `link` reports `expected`, letting other updates pass.
async fn settle(link: &mut HostLink, expected: HostConnection) {
    let mut seen = Vec::new();
    let reached = tokio::time::timeout(WAIT, async {
        while link.connection() != expected {
            seen.push(next_update(link).await);
        }
    })
    .await;
    assert!(
        reached.is_ok(),
        "never reached {expected:?}; now {:?} after {seen:?}",
        link.connection()
    );
}

/// A host joined with explicit credentials, before it says what it serves.
fn remote_host() -> HostFeatures {
    HostFeatures {
        remote: true,
        ..HostFeatures::default()
    }
}

fn controlling() -> HostConnection {
    HostConnection {
        role: ControlRole::Controller,
        connected: true,
        stopped: None,
        features: remote_host(),
    }
}

fn watching() -> HostConnection {
    HostConnection {
        role: ControlRole::Watcher,
        connected: true,
        stopped: None,
        features: remote_host(),
    }
}

/// A TCP relay that can drop every relayed connection while the host keeps
/// running, as a network outage would.
struct Relay {
    addr: std::net::SocketAddr,
    cut: watch::Sender<u64>,
}

impl Relay {
    async fn to(target: std::net::SocketAddr) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (cut, cuts) = watch::channel(0);
        tokio::spawn(async move {
            while let Ok((mut inbound, _)) = listener.accept().await {
                let mut cuts = cuts.clone();
                cuts.mark_unchanged();
                tokio::spawn(async move {
                    let Ok(mut outbound) = TcpStream::connect(target).await else {
                        return;
                    };
                    tokio::select! {
                        _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound) => {}
                        _ = cuts.changed() => {
                            let _ = inbound.shutdown().await;
                        }
                    }
                });
            }
        });
        Self { addr, cut }
    }

    fn drop_connections(&self) {
        self.cut.send_modify(|count| *count += 1);
    }
}

#[tokio::test]
async fn the_first_joined_terminal_controls_and_a_second_watches_until_it_takes_control() {
    let server = host().await;
    let endpoint = endpoint_of(server.local_addr());

    let mut first = join(endpoint.clone()).await;
    settle(&mut first, controlling()).await;
    let mut second = join(endpoint).await;
    settle(&mut second, watching()).await;
    assert_eq!(first.connection(), controlling());

    second.take_control();
    settle(&mut second, controlling()).await;
    settle(&mut first, watching()).await;

    server.shutdown().await;
}

#[tokio::test]
async fn a_dropped_connection_fails_unanswered_calls_and_comes_back_initialized() {
    let agent = Arc::<QuietHost>::default();
    let server = host_with(agent.clone()).await;
    let relay = Relay::to(server.local_addr()).await;
    let mut link = join(endpoint_of(relay.addr)).await;
    settle(&mut link, controlling()).await;

    let (reply, receipt) = tokio::sync::oneshot::channel();
    link.request(
        HostCall::FindPaths {
            session_id: None,
            query: "src".into(),
            kind: nori_protocol::PathKind::Files,
            limit: 1,
        },
        HostReply::Receipt(reply),
    );
    tokio::time::timeout(WAIT, agent.searching.notified())
        .await
        .expect("the search must reach the host");
    relay.drop_connections();

    while !matches!(next_update(&mut link).await, HostUpdate::Disconnected(_)) {}
    assert_eq!(
        link.connection(),
        HostConnection {
            role: ControlRole::Controller,
            connected: false,
            stopped: None,
            features: remote_host(),
        }
    );
    let error = settled(receipt)
        .await
        .expect_err("an unanswered call cannot succeed");
    assert!(
        error.to_string().contains("may have reached the host"),
        "{error}"
    );

    let reconnected = loop {
        if let HostUpdate::Ready { reconnected } = next_update(&mut link).await {
            break reconnected;
        }
    };
    assert!(reconnected);
    let connection = link.connection();
    assert!(
        connection.connected && connection.stopped.is_none(),
        "{connection:?}"
    );

    server.shutdown().await;
}

#[tokio::test]
async fn a_host_that_stops_leaves_the_joined_terminal_running_with_the_reason() {
    let original = host().await;
    let initial = ManagedClient::connect(&endpoint_of(original.local_addr()), TOKEN)
        .await
        .unwrap();
    // The reconnect target is another host instance, so the client must
    // treat its sessions as lost rather than reconnect to it.
    let replacement = host().await;
    let mut link = HostLink::join(
        HostEndpoint {
            endpoint: endpoint_of(replacement.local_addr()),
            token: TOKEN.into(),
            local_token: None,
            refresh_local_token: None,
        },
        initial,
    );
    settle(&mut link, controlling()).await;

    original.shutdown().await;
    let reason = loop {
        if let HostUpdate::Stopped(reason) = next_update(&mut link).await {
            break reason;
        }
    };
    assert!(reason.contains("Host restarted"), "{reason}");
    assert_eq!(link.connection().stopped, Some(reason.clone()));

    let (reply, receipt) = tokio::sync::oneshot::channel();
    link.request(HostCall::List, HostReply::Receipt(reply));
    assert_eq!(
        settled(receipt)
            .await
            .expect_err("a stopped host answers nothing")
            .to_string(),
        reason
    );

    replacement.shutdown().await;
}

#[tokio::test]
async fn a_joined_link_reports_the_hosts_session_list() {
    let server = host().await;
    let mut link = join(endpoint_of(server.local_addr())).await;

    let listed = tokio::time::timeout(WAIT, async {
        loop {
            if let HostUpdate::Sessions(list) = next_update(&mut link).await {
                break list;
            }
        }
    })
    .await
    .expect("the heartbeat must list the host's sessions");
    assert_eq!(listed, json!({}));

    server.shutdown().await;
}
