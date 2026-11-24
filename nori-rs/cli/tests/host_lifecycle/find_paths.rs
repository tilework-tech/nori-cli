//! `_nori/session/find_paths`: file and folder completion on the host's
//! filesystem, rooted at a session's folder or the host's own.

use super::acp_v1::connect;
use super::acp_v1::controller;
use super::acp_v1::host_with;
use super::acp_v1::new_session;
use super::acp_v1::watcher;
use super::*;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

/// A session folder with `src/alpha_needle.rs` and links leading out of it,
/// and a folder beside it holding `beta_needle.rs`.
fn folders() -> (TempDir, TempDir) {
    let inside = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    std::fs::create_dir(inside.path().join("src")).unwrap();
    std::fs::create_dir(inside.path().join("spec")).unwrap();
    std::fs::create_dir(inside.path().join(".secret")).unwrap();
    std::fs::write(inside.path().join("src/alpha_needle.rs"), "").unwrap();
    std::fs::write(outside.path().join("beta_needle.rs"), "").unwrap();
    std::os::unix::fs::symlink(outside.path(), inside.path().join("linked_dir")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("beta_needle.rs"),
        inside.path().join("linked_needle.rs"),
    )
    .unwrap();
    (inside, outside)
}

fn paths(result: &Value) -> Vec<&str> {
    result["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|found| found["path"].as_str().unwrap())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn initialize_advertises_find_paths() {
    let host = host_with("", &[]);
    let mut client = connect(&host);
    let result = client.request("initialize", json!({})).await.unwrap();
    assert_eq!(
        result["capabilities"]["nori_sessions"]["find_paths"],
        json!(true)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn files_are_found_in_the_sessions_folder_only() {
    let host = host_with("", &[]);
    let (inside, _outside) = folders();
    let elsewhere = TempDir::new().unwrap();
    std::fs::write(elsewhere.path().join("gamma_needle.rs"), "").unwrap();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, inside.path()).await;
    let (other, _) = new_session(&mut acp, elsewhere.path()).await;

    let found = acp
        .call(
            "_nori/session/find_paths",
            json!({"sessionId": id, "query": "needle", "kind": "files"}),
        )
        .await
        .unwrap();

    assert_eq!(found["root"], json!(canonical(inside.path())));
    assert_eq!(paths(&found), vec!["src/alpha_needle.rs"]);
    assert!(
        found["matches"][0]["indices"]
            .as_array()
            .is_some_and(|indices| !indices.is_empty()),
        "{found}"
    );
    let other_found = acp
        .call(
            "_nori/session/find_paths",
            json!({"sessionId": other, "query": "needle", "kind": "files", "limit": 5}),
        )
        .await
        .unwrap();
    assert_eq!(paths(&other_found), vec!["gamma_needle.rs"]);
    let empty = acp
        .call(
            "_nori/session/find_paths",
            json!({"sessionId": id, "query": "", "kind": "files"}),
        )
        .await
        .unwrap();
    assert_eq!(paths(&empty), Vec::<&str>::new());
}

#[tokio::test(flavor = "multi_thread")]
async fn folders_complete_in_the_form_they_were_typed() {
    let host = host_with("", &[]);
    let (inside, _outside) = folders();
    std::fs::create_dir_all(inside.path().join("src/core")).unwrap();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, inside.path()).await;
    let absolute = format!("{}/s", canonical(inside.path()));

    let mut completions = Vec::new();
    for query in ["sr", "s", "src/co", ".se", absolute.as_str(), "missing/x"] {
        let found = acp
            .call(
                "_nori/session/find_paths",
                json!({"sessionId": id, "query": query, "kind": "folders"}),
            )
            .await
            .unwrap();
        assert_eq!(found["matches"][0].get("indices"), None, "{found}");
        completions.push(
            paths(&found)
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>(),
        );
    }

    let root = canonical(inside.path());
    assert_eq!(
        completions,
        vec![
            vec!["src/".to_owned()],
            vec!["spec/".to_owned(), "src/".to_owned()],
            vec!["src/core/".to_owned()],
            vec![".secret/".to_owned()],
            vec![format!("{root}/spec/"), format!("{root}/src/")],
            Vec::new(),
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn without_a_session_the_hosts_own_folder_is_searched() {
    let folder = TempDir::new().unwrap();
    std::fs::write(folder.path().join("delta_needle.rs"), "").unwrap();
    let host = Host::new();
    success(bounded_output(
        host.command(&["host", "--port", "0"])
            .current_dir(folder.path()),
    ));
    let mut acp = controller(&host);

    let found = acp
        .call(
            "_nori/session/find_paths",
            json!({"query": "needle", "kind": "files"}),
        )
        .await
        .unwrap();

    assert_eq!(
        found,
        json!({
            "root": canonical(folder.path()),
            "matches": [{"path": "delta_needle.rs", "indices": found["matches"][0]["indices"]}],
        })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn links_leading_out_of_the_folder_are_not_returned() {
    let host = host_with("", &[]);
    let (inside, _outside) = folders();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, inside.path()).await;

    let found = acp
        .call(
            "_nori/session/find_paths",
            json!({"sessionId": id, "query": "beta", "kind": "files"}),
        )
        .await
        .unwrap();

    assert_eq!(paths(&found), Vec::<&str>::new());
}

#[tokio::test(flavor = "multi_thread")]
async fn watchers_may_search_and_unknown_sessions_are_refused() {
    let host = host_with("", &[]);
    let (inside, _outside) = folders();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, inside.path()).await;
    let mut watching = watcher(&host);

    let found = watching
        .call(
            "_nori/session/find_paths",
            json!({"sessionId": id, "query": "sr", "kind": "folders"}),
        )
        .await
        .unwrap();
    assert_eq!(paths(&found), vec!["src/"]);

    for session in ["not-a-session", "00000000-0000-0000-0000-000000000000"] {
        let refused = watching
            .call(
                "_nori/session/find_paths",
                json!({"sessionId": session, "query": "x", "kind": "files"}),
            )
            .await
            .unwrap_err();
        assert_eq!(
            (refused.code, refused.message.as_str()),
            (-32602, "unknown session"),
            "{session}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn new_session_folders_are_named_from_the_hosts_home_and_folder() {
    let home = TempDir::new().unwrap();
    std::fs::create_dir(home.path().join("orchard")).unwrap();
    let launch = TempDir::new().unwrap();
    std::fs::create_dir(launch.path().join("greenhouse")).unwrap();
    let host = Host::new();
    std::fs::write(
        host.0.path().join("config.toml"),
        "agent = \"mock-model\"\n",
    )
    .unwrap();
    success(bounded_output(
        host.command(&["host", "--port", "0"])
            .current_dir(launch.path())
            .env("HOME", home.path()),
    ));
    let mut acp = controller(&host);

    for (typed, expected) in [
        ("~", home.path().to_path_buf()),
        ("~/orchard", home.path().join("orchard")),
        ("greenhouse", launch.path().join("greenhouse")),
        ("./greenhouse", launch.path().join("greenhouse")),
    ] {
        let started = acp
            .call("_nori/session/new", json!({"cwd": typed}))
            .await
            .unwrap();
        assert_eq!(started["cwd"], json!(canonical(&expected)), "{typed}");
    }
    let refused = acp
        .call("_nori/session/new", json!({"cwd": "~nobody/orchard"}))
        .await
        .unwrap_err()
        .message;
    assert!(refused.contains("absolute"), "{refused}");
}
