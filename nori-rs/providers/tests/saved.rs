#![allow(clippy::unwrap_used)]

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use codex_keyring_store::tests::MockKeyringStore;
use common::ForbiddenKeyring;
use common::UnavailableKeyring;
use nori_acp_host::registry::AgentKind;
use nori_providers::STORE_MODE_ENV;
use nori_providers::SavedKeyStore;
use nori_providers::StoreLocation;
use nori_providers::StoreMode;
use pretty_assertions::assert_eq;

const KEY: &str = "sk-ant-api03-SECRET";

/// Every file under `dir` whose contents include `needle`, with its mode.
fn files_containing(dir: &Path, needle: &str) -> Vec<(String, u32)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(files_containing(&path, needle));
        } else if std::fs::read_to_string(&path)
            .unwrap_or_default()
            .contains(needle)
        {
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            found.push((
                path.file_name().unwrap().to_string_lossy().into_owned(),
                mode,
            ));
        }
    }
    found
}

#[test]
fn key_round_trips_through_the_keychain_without_touching_disk() {
    let dir = tempfile::tempdir().unwrap();
    let store = SavedKeyStore::new(
        dir.path(),
        StoreMode::Auto,
        Arc::new(MockKeyringStore::default()),
    );

    assert_eq!(
        store.save(AgentKind::ClaudeCode, KEY).unwrap(),
        StoreLocation::Keyring
    );
    assert_eq!(
        store.load(AgentKind::ClaudeCode).unwrap(),
        Some(KEY.to_string())
    );
    assert_eq!(
        store.index().unwrap().location(AgentKind::ClaudeCode),
        Some(StoreLocation::Keyring)
    );
    assert_eq!(files_containing(dir.path(), KEY), vec![]);
}

#[test]
fn unavailable_keychain_falls_back_to_a_private_file() {
    let dir = tempfile::tempdir().unwrap();
    let store = SavedKeyStore::new(dir.path(), StoreMode::Auto, Arc::new(UnavailableKeyring));

    assert_eq!(
        store.save(AgentKind::Codex, KEY).unwrap(),
        StoreLocation::File
    );
    assert_eq!(store.load(AgentKind::Codex).unwrap(), Some(KEY.to_string()));
    assert_eq!(
        store.index().unwrap().location(AgentKind::Codex),
        Some(StoreLocation::File)
    );
    let files = files_containing(dir.path(), KEY);
    assert_eq!(files.len(), 1, "{files:?}");
    assert_eq!(files[0].1, 0o600);
}

#[test]
fn keyring_mode_fails_instead_of_writing_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let store = SavedKeyStore::new(dir.path(), StoreMode::Keyring, Arc::new(UnavailableKeyring));

    let error = store.save(AgentKind::Codex, KEY).unwrap_err();
    assert!(!format!("{error:#}").contains(KEY));
    assert_eq!(files_containing(dir.path(), KEY), vec![]);
    assert_eq!(store.index().unwrap().location(AgentKind::Codex), None);
}

#[test]
fn file_mode_keeps_keys_out_of_the_keychain() {
    let dir = tempfile::tempdir().unwrap();
    let store = SavedKeyStore::new(dir.path(), StoreMode::File, Arc::new(ForbiddenKeyring));

    assert_eq!(
        store.save(AgentKind::Gemini, KEY).unwrap(),
        StoreLocation::File
    );
    assert_eq!(
        store.load(AgentKind::Gemini).unwrap(),
        Some(KEY.to_string())
    );
    assert!(store.delete(AgentKind::Gemini).unwrap());
}

#[test]
fn moving_a_key_into_the_keychain_removes_the_file_copy() {
    let dir = tempfile::tempdir().unwrap();
    SavedKeyStore::new(
        dir.path(),
        StoreMode::File,
        Arc::new(MockKeyringStore::default()),
    )
    .save(AgentKind::Codex, "old-SECRET")
    .unwrap();

    let store = SavedKeyStore::new(
        dir.path(),
        StoreMode::Auto,
        Arc::new(MockKeyringStore::default()),
    );
    store.save(AgentKind::Codex, KEY).unwrap();

    assert_eq!(store.load(AgentKind::Codex).unwrap(), Some(KEY.to_string()));
    assert_eq!(files_containing(dir.path(), "SECRET"), vec![]);
}

#[test]
fn delete_forgets_the_key_everywhere() {
    let dir = tempfile::tempdir().unwrap();
    let keyring = Arc::new(MockKeyringStore::default());
    let store = SavedKeyStore::new(dir.path(), StoreMode::Auto, keyring);
    store.save(AgentKind::ClaudeCode, KEY).unwrap();

    assert!(store.delete(AgentKind::ClaudeCode).unwrap());
    assert_eq!(store.load(AgentKind::ClaudeCode).unwrap(), None);
    assert_eq!(store.index().unwrap().location(AgentKind::ClaudeCode), None);
    assert!(!store.delete(AgentKind::ClaudeCode).unwrap());
}

#[test]
fn agents_without_an_api_key_variable_cannot_save_one() {
    let dir = tempfile::tempdir().unwrap();
    let store = SavedKeyStore::new(
        dir.path(),
        StoreMode::Auto,
        Arc::new(MockKeyringStore::default()),
    );
    let error = store.save(AgentKind::OpenCode, KEY).unwrap_err();
    assert!(format!("{error}").contains("own login"), "{error}");
    assert_eq!(files_containing(dir.path(), KEY), vec![]);
}

#[test]
fn blank_or_multiline_keys_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let store = SavedKeyStore::new(
        dir.path(),
        StoreMode::Auto,
        Arc::new(MockKeyringStore::default()),
    );
    for bad in ["   ", "a\nb"] {
        let error = store.save(AgentKind::Codex, bad).unwrap_err();
        assert!(
            format!("{error}").contains("single non-empty line"),
            "{error}"
        );
    }
    assert_eq!(store.index().unwrap().location(AgentKind::Codex), None);
}

#[test]
fn surrounding_whitespace_is_trimmed() {
    let dir = tempfile::tempdir().unwrap();
    let store = SavedKeyStore::new(
        dir.path(),
        StoreMode::Auto,
        Arc::new(MockKeyringStore::default()),
    );
    store.save(AgentKind::Codex, "  sk-abc\n").unwrap();
    assert_eq!(
        store.load(AgentKind::Codex).unwrap(),
        Some("sk-abc".to_string())
    );
}

#[test]
fn store_mode_comes_from_the_environment() {
    let parse = |value: Option<&str>| {
        StoreMode::from_env(&|name| {
            (name == STORE_MODE_ENV)
                .then(|| value.map(str::to_string))
                .flatten()
        })
        .ok()
    };
    assert_eq!(
        [
            parse(None),
            parse(Some("auto")),
            parse(Some("keyring")),
            parse(Some("file")),
            parse(Some("vault"))
        ],
        [
            Some(StoreMode::Auto),
            Some(StoreMode::Auto),
            Some(StoreMode::Keyring),
            Some(StoreMode::File),
            None
        ]
    );
}

#[test]
fn each_agent_keeps_its_own_key() {
    let dir = tempfile::tempdir().unwrap();
    let store = SavedKeyStore::new(
        dir.path(),
        StoreMode::Auto,
        Arc::new(MockKeyringStore::default()),
    );
    store.save(AgentKind::ClaudeCode, "claude-SECRET").unwrap();
    store.save(AgentKind::Codex, "codex-SECRET").unwrap();
    assert_eq!(
        [
            store.load(AgentKind::ClaudeCode).unwrap(),
            store.load(AgentKind::Codex).unwrap()
        ],
        [
            Some("claude-SECRET".to_string()),
            Some("codex-SECRET".to_string())
        ]
    );
}

#[test]
fn unreadable_keychain_reports_an_error_without_the_key() {
    let dir = tempfile::tempdir().unwrap();
    SavedKeyStore::new(
        dir.path(),
        StoreMode::Auto,
        Arc::new(MockKeyringStore::default()),
    )
    .save(AgentKind::Codex, KEY)
    .unwrap();
    let store = SavedKeyStore::new(dir.path(), StoreMode::Auto, Arc::new(UnavailableKeyring));

    let error = store.load(AgentKind::Codex).unwrap_err();
    assert!(!format!("{error:#}").contains(KEY));
}

#[test]
fn nothing_indexed_means_the_keychain_is_never_read() {
    let dir = tempfile::tempdir().unwrap();
    let store = SavedKeyStore::new(dir.path(), StoreMode::Auto, Arc::new(ForbiddenKeyring));
    assert_eq!(store.load(AgentKind::ClaudeCode).unwrap(), None);
}

#[test]
fn every_file_the_store_writes_is_private() {
    let dir = tempfile::tempdir().unwrap();
    let store = SavedKeyStore::new(dir.path(), StoreMode::File, Arc::new(ForbiddenKeyring));
    std::fs::write(dir.path().join("saved-keys.secret.json"), "{}").unwrap();
    std::fs::set_permissions(
        dir.path().join("saved-keys.secret.json"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    store.save(AgentKind::Codex, KEY).unwrap();

    let mut modes: Vec<(String, u32)> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".json"))
        .map(|entry| {
            let mode = entry.metadata().unwrap().permissions().mode() & 0o777;
            (entry.file_name().to_string_lossy().into_owned(), mode)
        })
        .collect();
    modes.sort();
    assert_eq!(
        modes,
        vec![
            ("saved-keys.json".to_string(), 0o600),
            ("saved-keys.secret.json".to_string(), 0o600),
        ]
    );
}

#[test]
fn corrupt_key_file_errors_without_echoing_its_contents() {
    let dir = tempfile::tempdir().unwrap();
    let store = SavedKeyStore::new(dir.path(), StoreMode::File, Arc::new(ForbiddenKeyring));
    store.save(AgentKind::Codex, KEY).unwrap();
    std::fs::write(
        dir.path().join("saved-keys.secret.json"),
        format!("{{\"codex\": {KEY}"),
    )
    .unwrap();

    let error = store.load(AgentKind::Codex).unwrap_err();
    assert!(!format!("{error:#}").contains(KEY), "{error:#}");
}

#[test]
fn concurrent_saves_for_different_agents_all_survive() {
    let dir = tempfile::tempdir().unwrap();
    let agents = [
        AgentKind::ClaudeCode,
        AgentKind::Codex,
        AgentKind::Gemini,
        AgentKind::GrokBuild,
        AgentKind::Cursor,
    ];
    std::thread::scope(|scope| {
        for agent in agents {
            let path = dir.path();
            scope.spawn(move || {
                let store = SavedKeyStore::new(path, StoreMode::File, Arc::new(ForbiddenKeyring));
                store
                    .save(agent, &format!("{}-SECRET", agent.slug()))
                    .unwrap();
            });
        }
    });

    let store = SavedKeyStore::new(dir.path(), StoreMode::File, Arc::new(ForbiddenKeyring));
    let loaded: Vec<_> = agents.map(|agent| store.load(agent).unwrap()).to_vec();
    let expected: Vec<_> = agents
        .map(|agent| Some(format!("{}-SECRET", agent.slug())))
        .to_vec();
    assert_eq!(loaded, expected);
}
