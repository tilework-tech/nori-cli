//! Startup ownership viewed through independent CLI processes in one Nori home.

use super::*;

#[test]
fn simultaneous_starts_report_one_ready_host_that_stays_stopped() {
    let host = Host::new();
    let gate = std::sync::Barrier::new(12);
    let replies = std::thread::scope(|scope| {
        (0..12)
            .map(|_| {
                scope.spawn(|| {
                    gate.wait();
                    success(bounded_output(
                        host.command(&["host", "--port", "0"])
                            .env("TOKIO_WORKER_THREADS", "2"),
                    ))
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|caller| caller.join().unwrap())
            .collect::<Vec<_>>()
    });
    let pid = discovery(&host)["pid"].as_i64().unwrap();
    assert!(
        replies
            .iter()
            .all(|reply| reply.contains(&format!("(pid {pid})"))),
        "{replies:?}"
    );
    assert_eq!(
        replies
            .iter()
            .filter(|reply| reply.starts_with("Host started"))
            .count(),
        1,
        "{replies:?}"
    );
    success(host.run(&["host", "ls"]));
    success(host.run(&["host", "stop"]));
    // Outwait the previous two-second server lock retry window.
    std::thread::sleep(Duration::from_millis(2200));
    assert!(!host.run(&["host", "ls"]).status.success());
}

#[test]
fn a_failed_cold_start_releases_ownership_for_the_next_caller() {
    let host = Host::new();
    let port = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let args = port.local_addr().unwrap().port().to_string();
    failure(host.run(&["host", "--port", &args]));
    drop(port);

    success(host.run(&["host", "--port", &args]));
    success(host.run(&["host", "ls"]));
}

#[test]
fn a_host_that_fails_authentication_is_never_reported_ready() {
    let host = Host::new();
    success(host.run(&["host", "--port", "0"]));
    let token_path = host.0.path().join("host/token");
    let token = std::fs::read(&token_path).unwrap();
    std::fs::write(&token_path, "different-token").unwrap();
    let attempted = host.run(&["host", "--port", "0"]);
    std::fs::write(&token_path, token).unwrap();

    failure(attempted);
    success(host.run(&["host", "ls"]));
}

#[test]
fn a_managed_host_can_reuse_a_crashed_ordinary_hosts_home() {
    let host = Host::new();
    // Retain the child handle so this test can kill and reap exactly the server
    // whose crash leaves ordinary discovery in this private home.
    let mut ordinary = host
        .command(&["host", "--foreground", "--port", "0"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_for_file(&host.0.path().join("host/state.json"));
    ordinary.kill().unwrap();
    ordinary.wait().unwrap();
    std::fs::create_dir(host.0.path().join("hosted-sessions")).unwrap();
    std::fs::write(
        host.0.path().join("config.toml"),
        "[host]\nmanaged = true\n",
    )
    .unwrap();

    let started = success(host.run(&["host", "--port", "0"]));
    assert!(started.contains("/sessions"), "{started}");
    assert!(!host.0.path().join("host/host.json").exists());
    let reused = success(host.run(&["host", "--port", "0"]));
    assert!(
        reused.contains("already running") && reused.contains("/sessions"),
        "{reused}"
    );
}

#[test]
fn foreground_start_succeeds_while_other_clients_check_for_a_host() {
    let host = Host::new();
    let readers = std::sync::Barrier::new(5);
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                readers.wait();
                for _ in 0..6 {
                    let _ = bounded_output(
                        host.command(&["host", "ls"])
                            .env("TOKIO_WORKER_THREADS", "2"),
                    );
                }
            });
        }
        readers.wait();
        let mut child = host
            .command(&["host", "--foreground", "--port", "0"])
            .env("TOKIO_WORKER_THREADS", "2")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let ready = loop {
            if host.run(&["host", "ls"]).status.success() {
                break true;
            }
            if child.try_wait().unwrap().is_some() || Instant::now() >= deadline {
                break false;
            }
        };
        let _ = child.kill();
        child.wait().unwrap();
        assert!(ready, "the foreground server never became reachable");
    });
}
