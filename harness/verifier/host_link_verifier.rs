//! Verifier-only checks of a joined HostLink against a real RemoteAcpServer.
//! Injected into a verifier tree as `app::host_link_verifier`; never shipped.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use nori_harness::remote_agent::ManagedAgent;
use nori_harness::remote_agent::ManagedClient;
use nori_harness::remote_agent::ManagedReply;
use nori_harness::remote_agent::RemoteAcpServer;
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
use crate::host_client::HostEndpoint;
use nori_protocol::ControlRole;

const TOKEN: &str = "verifier-token-7731";
const WAIT: Duration = Duration::from_secs(20);

struct Echo;
impl ManagedAgent for Echo {
    async fn request(&self, request: Value) -> ManagedReply {
        ManagedReply {
            delivery_guard: None,
            response: json!({"jsonrpc":"2.0","id":request["id"],"result":{}}),
            subscription: None,
        }
    }
}

async fn server() -> RemoteAcpServer {
    RemoteAcpServer::bind_host("127.0.0.1:0".parse().unwrap(), Arc::new(Echo), TOKEN.into())
        .await
        .unwrap()
}

fn ep(addr: std::net::SocketAddr) -> String {
    format!("ws://{addr}/sessions")
}

async fn joined(endpoint: String) -> HostLink {
    let initial = ManagedClient::connect(&endpoint, TOKEN).await.unwrap();
    HostLink::join(
        HostEndpoint { endpoint, token: TOKEN.into(), refresh_local_token: None },
        initial,
    )
}

async fn pump_until(link: &mut HostLink, mut done: impl FnMut(&HostLink, Option<&HostUpdate>) -> bool) -> Vec<String> {
    let mut seen = Vec::new();
    let ok = tokio::time::timeout(WAIT, async {
        if done(link, None) { return; }
        loop {
            let event = link.next().await.expect("link keeps running");
            let update = link.handle(event).expect("joined links never fail");
            if let Some(u) = &update { seen.push(format!("{u:?}")); }
            if done(link, update.as_ref()) { return; }
        }
    })
    .await;
    assert!(ok.is_ok(), "condition never reached; saw {seen:?}; now {:?}", link.connection());
    seen
}

/// Pump `link` for `dur`, returning every update it produced.
async fn pump_for(link: &mut HostLink, dur: Duration) -> Vec<String> {
    let mut seen = Vec::new();
    let _ = tokio::time::timeout(dur, async {
        loop {
            let event = link.next().await.expect("link keeps running");
            if let Some(u) = link.handle(event).expect("joined links never fail") {
                seen.push(format!("{u:?}"));
            }
        }
    })
    .await;
    seen
}

fn ready(l: &HostLink) -> bool {
    let c = l.connection();
    c.connected && c.stopped.is_none()
}

/// A relay that can cut live connections and refuse new ones while paused.
struct Gate {
    addr: std::net::SocketAddr,
    cut: watch::Sender<u64>,
    paused: Arc<AtomicBool>,
}
impl Gate {
    async fn to(target: std::net::SocketAddr) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (cut, cuts) = watch::channel(0u64);
        let paused = Arc::new(AtomicBool::new(false));
        let p = paused.clone();
        tokio::spawn(async move {
            while let Ok((mut inbound, _)) = listener.accept().await {
                if p.load(Ordering::SeqCst) {
                    let _ = inbound.shutdown().await;
                    continue;
                }
                let mut cuts = cuts.clone();
                cuts.mark_unchanged();
                tokio::spawn(async move {
                    let Ok(mut outbound) = TcpStream::connect(target).await else { return };
                    tokio::select! {
                        _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound) => {}
                        _ = cuts.changed() => { let _ = inbound.shutdown().await; }
                    }
                });
            }
        });
        Self { addr, cut, paused }
    }
    fn cut_and_pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
        self.cut.send_modify(|c| *c += 1);
    }
    fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn v1_control_moves_back_and_forth_between_two_joined_terminals() {
    let s = server().await;
    let mut a = joined(ep(s.local_addr())).await;
    pump_until(&mut a, |l, _| ready(l)).await;
    let mut b = joined(ep(s.local_addr())).await;
    pump_until(&mut b, |l, _| ready(l)).await;
    assert_eq!(a.connection().role, ControlRole::Controller);
    assert_eq!(b.connection().role, ControlRole::Watcher);
    for _ in 0..3 {
        b.take_control();
        pump_until(&mut b, |l, _| l.connection().role == ControlRole::Controller).await;
        pump_until(&mut a, |l, _| l.connection().role == ControlRole::Watcher).await;
        a.take_control();
        pump_until(&mut a, |l, _| l.connection().role == ControlRole::Controller).await;
        pump_until(&mut b, |l, _| l.connection().role == ControlRole::Watcher).await;
    }
    s.shutdown().await;
}

#[tokio::test]
async fn v2_a_call_while_disconnected_is_refused_at_once() {
    let s = server().await;
    let gate = Gate::to(s.local_addr()).await;
    let mut a = joined(ep(gate.addr)).await;
    pump_until(&mut a, |l, _| ready(l)).await;
    gate.cut_and_pause();
    pump_until(&mut a, |_, u| matches!(u, Some(HostUpdate::Disconnected(_)))).await;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let t = Instant::now();
    a.request(HostCall::List, HostReply::Receipt(tx));
    let err = tokio::time::timeout(Duration::from_secs(1), rx)
        .await
        .expect("refused without waiting for the connection")
        .unwrap()
        .expect_err("cannot succeed while down");
    assert!(err.to_string().contains("Reconnecting"), "{err}");
    assert!(t.elapsed() < Duration::from_millis(500));
    // Take-control while down must not queue a request that fires later.
    a.take_control();
    gate.resume();
    let seen = pump_until(&mut a, |_, u| matches!(u, Some(HostUpdate::Ready { reconnected: true }))).await;
    assert!(seen.iter().all(|u| !u.contains("Stopped")), "{seen:?}");
    s.shutdown().await;
}

#[tokio::test]
async fn v3_a_controller_that_reconnects_after_losing_control_comes_back_as_a_watcher() {
    let s = server().await;
    let gate = Gate::to(s.local_addr()).await;
    let mut a = joined(ep(gate.addr)).await;
    pump_until(&mut a, |l, _| ready(l)).await;
    assert_eq!(a.connection().role, ControlRole::Controller);
    let mut b = joined(ep(s.local_addr())).await;
    pump_until(&mut b, |l, _| ready(l)).await;
    gate.cut_and_pause();
    pump_until(&mut a, |_, u| matches!(u, Some(HostUpdate::Disconnected(_)))).await;
    b.take_control();
    pump_until(&mut b, |l, _| l.connection().role == ControlRole::Controller).await;
    gate.resume();
    pump_until(&mut a, |_, u| matches!(u, Some(HostUpdate::Ready { reconnected: true }))).await;
    assert_eq!(a.connection().role, ControlRole::Watcher, "role must come from the host on reconnect");
    assert_eq!(b.connection().role, ControlRole::Controller);
    // b must not have been demoted by a's return.
    let seen = pump_for(&mut b, Duration::from_millis(1500)).await;
    assert!(seen.iter().all(|u| !u.contains("Watcher")), "{seen:?}");
    s.shutdown().await;
}

#[tokio::test]
async fn v4_an_idle_joined_terminal_stays_connected_past_the_silence_timeout() {
    let s = server().await;
    let mut a = joined(ep(s.local_addr())).await;
    pump_until(&mut a, |l, _| ready(l)).await;
    let seen = pump_for(&mut a, Duration::from_secs(14)).await;
    let seen: Vec<_> = seen.into_iter().filter(|u| !u.starts_with("Sessions(")).collect();
    assert!(seen.is_empty(), "idle link produced updates: {seen:?}");
    assert!(ready(&a));
    s.shutdown().await;
}

async fn answered(link: &mut HostLink, call: HostCall) -> anyhow::Result<Value> {
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    link.request(call, HostReply::Receipt(tx));
    tokio::time::timeout(WAIT, async {
        loop {
            tokio::select! {
                r = &mut rx => return r.expect("reply delivered"),
                ev = link.next() => { let _ = link.handle(ev.expect("running")).unwrap(); }
            }
        }
    })
    .await
    .expect("call settled")
}

#[tokio::test]
async fn v5_leaving_closes_only_this_connection() {
    let s = server().await;
    let mut a = joined(ep(s.local_addr())).await;
    pump_until(&mut a, |l, _| ready(l)).await;
    let mut b = joined(ep(s.local_addr())).await;
    pump_until(&mut b, |l, _| ready(l)).await;
    a.stop_or_leave().await.unwrap();
    answered(&mut b, HostCall::List).await.expect("the host still answers the other terminal");
    b.take_control();
    pump_until(&mut b, |l, _| l.connection().role == ControlRole::Controller).await;
    let mut c = joined(ep(s.local_addr())).await;
    pump_until(&mut c, |l, _| ready(l)).await;
    assert_eq!(c.connection().role, ControlRole::Watcher);
    s.shutdown().await;
}
