/// `/new` in cloud mode starts a second cloud
/// session beside the first, and both are listed as Nori Cloud sessions.
#[test]
#[cfg(target_os = "linux")]
fn test_cloud_sessions_share_the_session_list() {
    let fake = FakeHandroll::new();
    let config = cloud_session_config(&fake)
        .with_mock_response("cloud answer");
    let mut session =
        TuiSession::spawn_with_config(30, 110, config).expect("Failed to spawn nori cloud");
    session
        .wait_for_text("Nori Cloud · Mock Default Model", TIMEOUT)
        .expect("the first cloud session should connect");
    std::thread::sleep(TIMEOUT_INPUT);
    session.submit_input("first cloud question").unwrap();
    session
        .wait_for_text("cloud answer", TIMEOUT)
        .expect("the first cloud session answers");
    std::thread::sleep(Duration::from_millis(500));

    session.submit_input("/new").unwrap();
    fake.wait_for_child_pid_count(2, TIMEOUT);
    session
        .wait_for(
            |screen| !screen.contains("first cloud question") && screen.contains("2 ready"),
            TIMEOUT,
        )
        .expect("a second cloud session starts beside the first");
    std::thread::sleep(TIMEOUT_INPUT);
    session.submit_input("second cloud question").unwrap();
    session
        .wait_for_text("cloud answer", TIMEOUT)
        .expect("the second cloud session answers");

    session.send_key(Key::Alt('s')).unwrap();
    session
        .wait_for(
            |screen| {
                screen.contains("Sessions 2")
                    && screen.contains("first cloud question")
                    && screen.contains("second cloud question")
                    && screen
                        .lines()
                        .filter(|line| {
                            line.contains("cloud question") && line.contains("Nori Cloud")
                        })
                        .count()
                        == 2
            },
            Duration::from_secs(3),
        )
        .expect("both cloud sessions are listed");
    let pids = fake.wait_for_child_pid_count(2, Duration::from_millis(100));
    assert!(
        pids.iter().all(|pid| FakeHandroll::child_is_alive(*pid)),
        "both cloud children stay alive: {pids:?}"
    );
}

/// With several cloud sessions, `/close` releases the session on screen,
/// shows the next one, and lists the released one as closed.
#[test]
#[cfg(target_os = "linux")]
fn test_cloud_close_beside_another_session_lists_it_as_closed() {
    let fake = FakeHandroll::new();
    let config = cloud_lifecycle_config(&fake)
        .with_mock_response("cloud answer");
    let mut session =
        TuiSession::spawn_with_config(30, 110, config).expect("Failed to spawn nori cloud");
    session
        .wait_for_text("Start a new session", TIMEOUT)
        .expect("cloud entry should open the session picker");
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_until_ready()
        .expect("create-new should start a session");
    std::thread::sleep(TIMEOUT_INPUT);
    session.submit_input("first cloud question").unwrap();
    session
        .wait_for_text("cloud answer", TIMEOUT)
        .expect("the first cloud session answers");
    std::thread::sleep(Duration::from_millis(500));

    session.submit_input("/new").unwrap();
    session
        .wait_for(
            |screen| !screen.contains("first cloud question") && screen.contains("2 ready"),
            TIMEOUT,
        )
        .expect("a second cloud session starts beside the first");
    std::thread::sleep(TIMEOUT_INPUT);
    session.submit_input("second cloud question").unwrap();
    session
        .wait_for_text("cloud answer", TIMEOUT)
        .expect("the second cloud session answers");
    std::thread::sleep(Duration::from_millis(500));

    session.submit_input("/close").unwrap();
    session
        .wait_for(
            |screen| screen.contains("first cloud question") && !screen.contains("Start a new"),
            TIMEOUT,
        )
        .expect("/close shows the remaining session");
    std::thread::sleep(Duration::from_millis(500));
    session.send_key(Key::Alt('s')).unwrap();
    session
        .wait_for_text("Sessions 2", Duration::from_secs(3))
        .expect("the list opens");
    std::thread::sleep(Duration::from_millis(300));
    let screen = session.screen_contents();
    assert!(
        screen
            .lines()
            .any(|line| line.contains("second cloud question") && line.contains("closed")),
        "the released session is listed as closed:\n{screen}"
    );
    assert!(!screen.contains("starting"), "{screen}");
    assert!(!screen.contains("New session"), "{screen}");
}
