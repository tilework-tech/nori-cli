#![allow(clippy::unwrap_used)]

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use common::UnavailableKeyring;
use common::file_store;
use common::keyring_store;
use nori_providers::Credential;
use nori_providers::CredentialStore;
use nori_providers::Provider;
use nori_providers::STORE_BACKEND_ENV;
use nori_providers::StoreBackend;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

const KEY: &str = "sk-proj-SECRET";

fn api_key(key: &str) -> Credential {
    Credential::api_key(key).unwrap()
}

/// Every file under `dir` whose contents include `needle`, with its mode.
fn files_containing(dir: &Path, needle: &str) -> Vec<(String, u32)> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
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

fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn credentials_json(dir: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(dir.join("credentials.json")).unwrap()).unwrap()
}

#[test]
fn default_store_is_one_private_file_of_typed_records_keyed_by_provider() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(dir.path());

    store.set(Provider::OpenAi, api_key(KEY)).unwrap();

    assert_eq!(
        store.get(Provider::OpenAi).unwrap(),
        Some(api_key(KEY)),
        "round trip"
    );
    assert_eq!(
        credentials_json(dir.path()),
        json!({"openai": {"type": "api_key", "key": KEY}})
    );
    assert_eq!(file_names(dir.path()), vec!["credentials.json"]);
    assert_eq!(
        files_containing(dir.path(), KEY),
        vec![("credentials.json".to_string(), 0o600)]
    );
}

#[test]
fn backend_defaults_to_the_file_and_keyring_is_opt_in() {
    let parse = |value: Option<&str>| {
        StoreBackend::from_env(&|name| {
            (name == STORE_BACKEND_ENV)
                .then(|| value.map(str::to_string))
                .flatten()
        })
        .ok()
    };
    assert_eq!(
        [
            parse(None),
            parse(Some("")),
            parse(Some("file")),
            parse(Some("keyring")),
            parse(Some("auto")),
            parse(Some("vault")),
        ],
        [
            Some(StoreBackend::File),
            Some(StoreBackend::File),
            Some(StoreBackend::File),
            Some(StoreBackend::Keyring),
            None,
            None,
        ]
    );
}

#[test]
fn keyring_backend_keeps_every_secret_off_disk() {
    let dir = tempfile::tempdir().unwrap();
    let store = keyring_store(dir.path());

    store.set(Provider::Anthropic, api_key(KEY)).unwrap();

    assert_eq!(store.get(Provider::Anthropic).unwrap(), Some(api_key(KEY)));
    assert_eq!(
        store.index().unwrap().providers(),
        vec![(Provider::Anthropic, "api_key".to_string())]
    );
    assert_eq!(files_containing(dir.path(), KEY), vec![]);
}

#[test]
fn unavailable_keyring_is_an_error_that_does_not_echo_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let store = CredentialStore::new(
        dir.path(),
        StoreBackend::Keyring,
        Arc::new(UnavailableKeyring),
    );

    let error = store.set(Provider::OpenAi, api_key(KEY)).unwrap_err();

    assert!(!format!("{error:#}").contains(KEY), "{error:#}");
    assert_eq!(files_containing(dir.path(), KEY), vec![]);
}

#[test]
fn remove_forgets_only_that_provider() {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(dir.path());
    store
        .set(Provider::Anthropic, api_key("anthropic-SECRET"))
        .unwrap();
    store
        .set(Provider::OpenAi, api_key("openai-SECRET"))
        .unwrap();

    assert!(store.remove(Provider::Anthropic).unwrap());
    assert!(!store.remove(Provider::Anthropic).unwrap());

    assert_eq!(
        [
            store.get(Provider::Anthropic).unwrap(),
            store.get(Provider::OpenAi).unwrap()
        ],
        [None, Some(api_key("openai-SECRET"))]
    );
}

#[test]
fn blank_or_multiline_keys_are_rejected_and_whitespace_is_trimmed() {
    for bad in ["", "   ", "a\nb"] {
        let error = Credential::api_key(bad).unwrap_err();
        assert!(
            format!("{error}").contains("single non-empty line"),
            "{error}"
        );
    }
    assert_eq!(Credential::api_key("  sk-abc\n").unwrap().key(), "sk-abc");
}

#[test]
fn credential_debug_output_hides_the_key() {
    assert!(!format!("{:?}", api_key(KEY)).contains(KEY));
}

#[test]
fn rewriting_a_loose_file_makes_it_private() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("credentials.json");
    std::fs::write(&path, "{}").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

    file_store(dir.path())
        .set(Provider::Xai, api_key(KEY))
        .unwrap();

    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn corrupt_file_errors_without_echoing_its_contents() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("credentials.json"),
        format!("{{\"openai\": {KEY}"),
    )
    .unwrap();

    let error = file_store(dir.path()).get(Provider::OpenAi).unwrap_err();

    assert!(!format!("{error:#}").contains(KEY), "{error:#}");
}

#[test]
fn records_of_unknown_types_are_listed_and_kept_but_not_used() {
    let dir = tempfile::tempdir().unwrap();
    let oauth = json!({"type": "oauth", "refresh": "r-SECRET", "access": "a-SECRET", "expires": 1});
    std::fs::write(
        dir.path().join("credentials.json"),
        json!({"anthropic": oauth}).to_string(),
    )
    .unwrap();
    let store = file_store(dir.path());

    store.set(Provider::OpenAi, api_key(KEY)).unwrap();

    assert_eq!(store.get(Provider::Anthropic).unwrap(), None);
    assert_eq!(
        store.index().unwrap().providers(),
        vec![
            (Provider::Anthropic, "oauth".to_string()),
            (Provider::OpenAi, "api_key".to_string()),
        ]
    );
    assert_eq!(credentials_json(dir.path())["anthropic"], oauth);
}

#[test]
fn concurrent_saves_for_different_providers_all_survive() {
    let dir = tempfile::tempdir().unwrap();
    let providers = Provider::all();
    std::thread::scope(|scope| {
        for provider in providers {
            let path = dir.path();
            scope.spawn(move || {
                file_store(path)
                    .set(*provider, api_key(&format!("{}-SECRET", provider.id())))
                    .unwrap();
            });
        }
    });

    let store = file_store(dir.path());
    let loaded: Vec<_> = providers
        .iter()
        .map(|provider| store.get(*provider).unwrap())
        .collect();
    let expected: Vec<_> = providers
        .iter()
        .map(|provider| Some(api_key(&format!("{}-SECRET", provider.id()))))
        .collect();
    assert_eq!(loaded, expected);
    assert_eq!(file_names(dir.path()), vec!["credentials.json"]);
}

#[test]
fn keys_saved_by_the_earlier_per_agent_format_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("saved-keys.json"), r#"{"codex":"file"}"#).unwrap();
    let store = file_store(dir.path());

    let errors = [
        store.get(Provider::OpenAi).map(|_| ()).unwrap_err(),
        store.index().map(|_| ()).unwrap_err(),
        store.set(Provider::OpenAi, api_key(KEY)).unwrap_err(),
    ];

    for error in errors {
        let message = format!("{error:#}");
        assert!(message.contains("saved-keys.json"), "{message}");
        assert!(message.contains("set-key"), "{message}");
    }
    assert!(!dir.path().join("credentials.json").exists());
}

#[test]
fn provider_ids_parse_and_agent_slugs_point_at_their_provider() {
    let parsed: Vec<_> = Provider::all()
        .iter()
        .map(|provider| Provider::parse(provider.id()).unwrap())
        .collect();
    assert_eq!(parsed, Provider::all().to_vec());

    let error = Provider::parse("codex").unwrap_err().to_string();
    assert!(error.contains("openai"), "{error}");
    let error = Provider::parse("hermes").unwrap_err().to_string();
    assert!(error.contains("own login"), "{error}");
    let error = Provider::parse("sk-pasted-SECRET").unwrap_err().to_string();
    assert!(error.contains("not a provider"), "{error}");
    assert!(!error.contains("SECRET"), "{error}");
}

#[test]
fn keyring_backend_removing_the_last_provider_deletes_the_item() {
    let dir = tempfile::tempdir().unwrap();
    let store = keyring_store(dir.path());
    store.set(Provider::Anthropic, api_key("a-SECRET")).unwrap();
    store.set(Provider::OpenAi, api_key("o-SECRET")).unwrap();

    assert!(store.remove(Provider::Anthropic).unwrap());
    assert_eq!(
        store.get(Provider::OpenAi).unwrap(),
        Some(api_key("o-SECRET"))
    );
    assert!(store.remove(Provider::OpenAi).unwrap());

    assert_eq!(store.index().unwrap().providers(), vec![]);
    assert_eq!(files_containing(dir.path(), "SECRET"), vec![]);
}
