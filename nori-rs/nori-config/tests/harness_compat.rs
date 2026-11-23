use anyhow::Context;
use nori_config::harness_compat::Contract;
use nori_config::harness_compat::Decision;
use nori_config::harness_compat::Environment;
use nori_config::harness_compat::Pairing;
use nori_config::harness_compat::Status;
use nori_config::harness_compat::embedded;
use nori_config::token_providers::Protocol;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;

const DOCUMENT: &str = include_str!("../data/harness-compat.json");

const RELEASE_NOTE: &str = include_str!("../data/harness-compat-release-note.md");

fn check_release_floor(revision: i64, note: &str) -> anyhow::Result<()> {
    let minimum: i64 = note
        .trim_end()
        .strip_prefix("Minimum harness-compat revision: ")
        .context("expected the harness-compat minimum in the release note")?
        .parse()?;
    anyhow::ensure!(minimum >= 0, "negative harness-compat minimum");
    anyhow::ensure!(
        revision >= minimum,
        "embedded harness-compat revision {revision} is older than release-note minimum {minimum}"
    );
    Ok(())
}

#[test]
fn embedded_contract_meets_release_note_minimum() -> anyhow::Result<()> {
    check_release_floor(embedded()?.revision(), RELEASE_NOTE)
}

#[test]
fn release_floor_accepts_current_and_newer_copies_and_rejects_stale_ones() -> anyhow::Result<()> {
    let note = "Minimum harness-compat revision: 8";
    for revision in [7, 8, 9] {
        let mut doc: Value = serde_json::from_str(DOCUMENT)?;
        doc["revision"] = json!(revision);
        let contract = Contract::read(&doc.to_string())?;
        assert_eq!(
            check_release_floor(contract.revision(), note).is_ok(),
            revision >= 8
        );
    }
    Ok(())
}

#[test]
fn pinned_document_accepts_new_v1_revisions_only() {
    assert_eq!(
        format!("{:x}", Sha256::digest(DOCUMENT)),
        "d9d12f4dcddaa13198189e07c567515031de613489bcbbb804429a23e0f75aec"
    );
    let mut doc: Value = serde_json::from_str(DOCUMENT).unwrap();
    doc["revision"] = json!(2731);
    assert_eq!(Contract::read(&doc.to_string()).unwrap().revision(), 2731);
    for version in [json!(0), json!(2), json!("1")] {
        doc["version"] = version;
        assert!(Contract::read(&doc.to_string()).is_err());
    }
}

#[test]
fn exact_custom_and_native_evidence() -> anyhow::Result<()> {
    use Protocol::OpenaiCompletions;
    use Protocol::OpenaiResponses;
    for (harness, protocol, status, reason) in [
        ("pi-acp", OpenaiResponses, Status::Unverified, None),
        (
            "hermes",
            OpenaiResponses,
            Status::Excluded,
            Some("protocol-mismatch"),
        ),
        ("hermes", OpenaiCompletions, Status::Unverified, None),
    ] {
        let result = embedded()?.evaluate(Pairing {
            harness,
            source: "token:custom",
            environment: Environment::Local,
            protocol: Some(protocol),
            exact: true,
            model: None,
            trust_saved_protocol: false,
        });
        assert_eq!(
            result,
            Decision {
                environment: Environment::Local,
                status,
                reason,
                protocol: reason.is_none().then_some(protocol),
                evidence: None
            }
        );
    }
    let document: Value = serde_json::from_str(DOCUMENT)?;
    for (harness, source) in [
        ("claude-acp", "native:anthropic-subscription"),
        ("codex-acp", "native:chatgpt-subscription"),
    ] {
        let decision = embedded()?.evaluate(Pairing {
            harness,
            source,
            environment: Environment::Local,
            protocol: None,
            exact: false,
            model: None,
            trust_saved_protocol: false,
        });
        let rule = document["rules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["scope"]["source"] == source && r["status"] == "supported")
            .unwrap();
        assert_eq!(
            decision,
            Decision {
                environment: Environment::Local,
                status: Status::Supported,
                reason: None,
                protocol: None,
                evidence: Some(&rule["evidence"])
            }
        );
    }
    for (harness, source) in [
        ("unknown-agent", "token:custom"),
        ("codex", "unknown-source"),
    ] {
        let decision = embedded()?.evaluate(Pairing {
            harness,
            source,
            environment: Environment::Managed,
            protocol: None,
            exact: false,
            model: None,
            trust_saved_protocol: false,
        });
        assert_eq!(
            decision,
            Decision {
                environment: Environment::Managed,
                status: Status::Excluded,
                reason: Some("protocol-mismatch"),
                protocol: None,
                evidence: None
            }
        );
    }
    Ok(())
}

#[test]
fn cli_only_aliases_use_canonical_ids_for_native_eligibility() -> anyhow::Result<()> {
    let contract = embedded()?;
    for (alias, id, source) in [
        ("hermes-agent", "hermes", "native:anthropic-api-key"),
        ("cursor-agent", "cursor", "native:cursor-api-key"),
    ] {
        let query = Pairing {
            harness: id,
            source,
            environment: Environment::Local,
            protocol: None,
            model: None,
            exact: false,
            trust_saved_protocol: false,
        };
        assert_eq!(contract.evaluate(query).status, Status::Unverified);
        let raw = contract.evaluate(Pairing {
            harness: alias,
            ..query
        });
        assert_eq!(
            (raw.status, raw.reason),
            (Status::Excluded, Some("protocol-mismatch"))
        );
    }
    Ok(())
}

fn visit_routes(harness: &str, source: &str, is_token: bool, mut visit: impl FnMut(Pairing<'_>)) {
    let protocols = [
        None,
        Some(Protocol::OpenaiCompletions),
        Some(Protocol::OpenaiResponses),
        Some(Protocol::AnthropicMessages),
    ];
    let wires = if is_token {
        &protocols[..]
    } else {
        &protocols[..1]
    };
    let flags: &[bool] = if is_token { &[false, true] } else { &[false] };
    let models: &[Option<&str>] = if source == "token:chatgpt" {
        &[
            None,
            Some("fixture-model"),
            Some("gpt-5.5"),
            Some("gpt-5.6"),
            Some("gpt-5.6-codex"),
            Some("gpt-5.7"),
            Some("gpt-6"),
        ]
    } else {
        &[None]
    };
    for environment in [Environment::Local, Environment::Managed] {
        for &protocol in wires {
            for &exact in flags {
                for &trust_saved_protocol in flags {
                    for &model in models {
                        visit(Pairing {
                            harness,
                            source,
                            environment,
                            protocol,
                            exact,
                            trust_saved_protocol,
                            model,
                        });
                    }
                }
            }
        }
    }
}

fn visit_matrix(doc: &Value, mut visit: impl FnMut(Pairing<'_>)) -> anyhow::Result<()> {
    for harness in doc["harnesses"]
        .as_array()
        .context("contract harnesses array")?
    {
        let mut names = Vec::new();
        for name in [&harness["id"], &harness["sessionsId"], &harness["cliSlug"]]
            .into_iter()
            .chain(
                harness["aliases"]
                    .as_array()
                    .context("contract aliases array")?,
            )
            .filter_map(Value::as_str)
        {
            if !names.contains(&name) {
                names.push(name);
            }
        }
        for name in names {
            for source in doc["sources"]
                .as_array()
                .context("contract sources array")?
            {
                visit_routes(
                    name,
                    source["id"].as_str().context("source id")?,
                    source["kind"] == "token",
                    &mut visit,
                );
            }
        }
    }
    Ok(())
}

// Each line of the broker digest identifies both the input and its decision.
#[test]
fn broker_decisions_across_aliases_routes_and_models() -> anyhow::Result<()> {
    let doc: Value = serde_json::from_str(DOCUMENT)?;
    let contract = embedded()?;
    let mut digest = Sha256::new();
    let mut counts = [0; 3];
    visit_matrix(&doc, |q| {
        let decision = contract.evaluate(q);
        counts[match decision.status {
            Status::Supported => 0,
            Status::Unverified => 1,
            Status::Excluded => 2,
        }] += 1;
        let row = json!([
            q.harness,
            q.source,
            q.environment,
            q.protocol,
            q.exact,
            q.trust_saved_protocol,
            q.model,
            decision.status,
            decision.protocol,
            decision.reason,
            decision.evidence.map(|e| &e["runId"])
        ]);
        digest.update(row.to_string());
        digest.update(b"\n");
    })?;
    assert_eq!(counts, [12, 1822, 5956]);
    assert_eq!(
        format!("{:x}", digest.finalize()),
        "db04910afd3940dbed1bd305d13eed2d5cd31839a951d05dc767f578c9b487db"
    );
    Ok(())
}
