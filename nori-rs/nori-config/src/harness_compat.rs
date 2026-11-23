//! Harness eligibility from the shared Sessions contract. Launchers own adapter availability.

use std::sync::LazyLock;

use anyhow::Result;
use anyhow::ensure;
use regex::Regex;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde_json::Value;

use crate::token_providers::Protocol;

/// Source of data/harness-compat.json in tilework-tech/nori-sessions,
/// broker/server/src/shared-definitions/harness-compat.json.
pub const SOURCE_COMMIT: &str = "f2c787a47128f8dff118fb327906d1f09effc795";

static CONTRACT: LazyLock<Result<Contract>> =
    LazyLock::new(|| Contract::read(include_str!("../data/harness-compat.json")));

pub fn embedded() -> Result<&'static Contract> {
    CONTRACT
        .as_ref()
        .map_err(|error| anyhow::anyhow!("{error:#}"))
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    Local,
    Managed,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Supported,
    Unverified,
    Excluded,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Native,
    Token,
}

pub struct Contract(Document);

#[derive(Deserialize)]
struct Document {
    version: f64,
    revision: i64,
    harnesses: Vec<Harness>,
    sources: Vec<Source>,
    rules: Vec<Rule>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Harness {
    id: String,
    sessions_id: Option<String>,
    cli_slug: String,
    aliases: Vec<String>,
    protocols: Vec<Protocol>,
    native_sources: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Source {
    id: String,
    kind: Kind,
    pub(crate) protocols: Vec<Protocol>,
    protocol_selection: Option<ProtocolSelection>,
    pub(crate) base_url: Option<String>,
    pub(crate) messages_auth: Option<String>,
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum ProtocolSelection {
    SavedOnly,
    SavedFirst,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Rule {
    scope: Scope,
    environments: Vec<Environment>,
    status: Status,
    reason: Option<String>,
    evidence: Option<Value>,
    #[serde(default)]
    provisional: bool,
    model_filter: Option<ModelFilter>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Scope {
    harness: Option<String>,
    source: Option<String>,
    kind: Option<Kind>,
    protocol: Option<Protocol>,
    #[serde(default)]
    except_harnesses: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelFilter {
    deny_exact: Vec<String>,
    #[serde(deserialize_with = "read_regex")]
    version_prefix: Regex,
    greater_than: [f64; 2],
}

fn read_regex<'de, D: Deserializer<'de>>(de: D) -> Result<Regex, D::Error> {
    Regex::new(&String::deserialize(de)?).map_err(serde::de::Error::custom)
}

impl ModelFilter {
    fn rejects(&self, model: &str) -> bool {
        if self.deny_exact.iter().any(|id| id == model) {
            return true;
        }
        let Some(captures) = self.version_prefix.captures(model) else {
            return true;
        };
        // The broker compares numeric major/minor captures, with an omitted minor of zero.
        let number = |index| {
            captures
                .get(index)
                .map_or(0.0, |m| m.as_str().parse::<f64>().unwrap_or(f64::NAN))
        };
        (number(1), number(2)) <= (self.greater_than[0], self.greater_than[1])
    }
}

#[derive(Clone, Copy)]
pub struct Pairing<'a> {
    /// A harness ID or alias declared by the broker contract. Resolve CLI
    /// command aliases such as `hermes-agent` and `cursor-agent` to their
    /// canonical IDs (`hermes` and `cursor`) before querying eligibility.
    pub harness: &'a str,
    pub source: &'a str,
    pub environment: Environment,
    pub protocol: Option<Protocol>,
    pub model: Option<&'a str>,
    pub exact: bool,
    /// Broker compatibility for already-saved declarations. New CLI entries use false.
    pub trust_saved_protocol: bool,
}

#[derive(Debug, PartialEq)]
pub struct Decision<'a> {
    pub environment: Environment,
    pub status: Status,
    pub protocol: Option<Protocol>,
    pub reason: Option<&'a str>,
    pub evidence: Option<&'a Value>,
}

impl Contract {
    pub fn read(document: &str) -> Result<Self> {
        let mut contract: Document = serde_json::from_str(document)?;
        ensure!(
            contract.version == 1.0,
            "unknown harness-compat version {}",
            contract.version
        );
        // Stable order breaks ties between rules with equal specificity.
        contract.rules.sort_by_key(|rule| {
            std::cmp::Reverse(
                4 * i32::from(rule.scope.harness.is_some())
                    + 2 * i32::from(rule.scope.source.is_some())
                    + i32::from(rule.scope.protocol.is_some()),
            )
        });
        Ok(Self(contract))
    }

    pub fn revision(&self) -> i64 {
        self.0.revision
    }

    fn harness(&self, id: &str) -> Option<&Harness> {
        self.0.harnesses.iter().find(|h| {
            h.id == id
                || h.sessions_id.as_deref() == Some(id)
                || h.cli_slug == id
                || h.aliases.iter().any(|alias| alias == id)
        })
    }

    pub fn cli_slug(&self, id: &str) -> Option<&str> {
        self.harness(id).map(|h| h.cli_slug.as_str())
    }

    pub(crate) fn source(&self, id: &str) -> Option<&Source> {
        self.0.sources.iter().find(|source| source.id == id)
    }

    pub fn evaluate(&self, query: Pairing<'_>) -> Decision<'_> {
        let excluded = |reason| Decision {
            environment: query.environment,
            status: Status::Excluded,
            protocol: None,
            reason: Some(reason),
            evidence: None,
        };
        let mismatch = "protocol-mismatch";
        let (Some(harness), Some(source)) =
            (self.harness(query.harness), self.source(query.source))
        else {
            return excluded(mismatch);
        };
        if source.kind == Kind::Native && !harness.native_sources.contains(&source.id) {
            return excluded("native-only");
        }

        let decide = |protocol| {
            let rule = self.0.rules.iter().find(|rule| {
                let scope = &rule.scope;
                rule.environments.contains(&query.environment)
                    && scope.harness.as_ref().is_none_or(|id| id == &harness.id)
                    && scope.source.as_ref().is_none_or(|id| id == &source.id)
                    && scope.kind.is_none_or(|kind| kind == source.kind)
                    && !scope.except_harnesses.contains(&harness.id)
                    && (scope.protocol.is_none() || scope.protocol == protocol)
                    && rule
                        .model_filter
                        .as_ref()
                        .is_none_or(|f| query.model.is_some_and(|id| f.rejects(id)))
            });
            let Some(rule) = rule else {
                return excluded(mismatch);
            };
            if rule.status == Status::Excluded && rule.scope.protocol.is_none() {
                return excluded(rule.reason.as_deref().unwrap_or(mismatch));
            }
            if rule.provisional {
                if query.exact && rule.scope.protocol.is_none() {
                    return excluded(mismatch);
                }
            } else if source.kind == Kind::Token
                && !protocol.is_some_and(|wire| {
                    harness.protocols.contains(&wire)
                        && (source.protocols.contains(&wire)
                            || (query.trust_saved_protocol && query.protocol == Some(wire)))
                })
            {
                return excluded(mismatch);
            }
            if rule.status == Status::Excluded {
                return excluded(rule.reason.as_deref().unwrap_or(mismatch));
            }
            Decision {
                environment: query.environment,
                status: rule.status,
                protocol: if rule.provisional {
                    rule.scope.protocol
                } else {
                    protocol
                },
                reason: None,
                evidence: rule.evidence.as_ref(),
            }
        };
        if source.kind == Kind::Native {
            return decide(None);
        }
        let defaults =
            !query.exact && source.protocol_selection == Some(ProtocolSelection::SavedFirst);
        let mut candidates = query
            .protocol
            .into_iter()
            .chain(
                source
                    .protocols
                    .iter()
                    .copied()
                    .filter(|wire| defaults && Some(*wire) != query.protocol),
            )
            .peekable();
        if candidates.peek().is_none() {
            return decide(None);
        }
        let mut rejection = excluded(mismatch);
        for protocol in candidates {
            let decision = decide(Some(protocol));
            if decision.status != Status::Excluded {
                return decision;
            }
            if rejection.reason == Some(mismatch) {
                rejection = decision;
            }
        }
        rejection
    }
}
