//! `nori providers`: per-agent sign-in status and API keys saved through Nori.

use std::io::IsTerminal;
use std::io::Read;
use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use codex_keyring_store::DefaultKeyringStore;
use nori_harness::registry::AgentKind;
use nori_providers::AuthStatus;
use nori_providers::CredentialKind;
use nori_providers::CredentialSource;
use nori_providers::DetectContext;
use nori_providers::Platform;
use nori_providers::SavedKeyStore;
use nori_providers::StoreLocation;
use nori_providers::StoreMode;
use nori_providers::SystemKeychain;
use serde_json::json;

#[derive(Debug, Parser)]
pub(crate) struct ProvidersCommand {
    #[command(subcommand)]
    action: Option<ProvidersAction>,
    /// Print the full report as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, clap::Subcommand)]
enum ProvidersAction {
    /// Save an API key for an agent, read from stdin, in the OS keychain (or a
    /// private file when no keychain is available). Nori passes it to the agent
    /// at launch unless the variable is already set.
    SetKey {
        /// Agent: claude-code, codex, gemini (Antigravity), grok-build, or cursor.
        agent: String,
        /// Print `{"agent", "env", "store"}`.
        #[arg(long)]
        json: bool,
    },
    /// Forget the API key saved for an agent.
    RemoveKey {
        /// Agent whose saved key to remove.
        agent: String,
    },
}

pub(crate) fn run(command: ProvidersCommand) -> anyhow::Result<()> {
    let env = |key: &str| std::env::var(key).ok();
    let nori_home = nori_config::find_nori_home()?;
    let store = SavedKeyStore::new(
        &nori_home,
        StoreMode::from_env(&env)?,
        Arc::new(DefaultKeyringStore),
    );
    let parse_agent = |slug: &str| {
        AgentKind::from_slug(slug)
            .with_context(|| format!("{slug:?} is not a built-in agent; see `nori providers`"))
    };
    match command.action {
        None => {
            let home = dirs::home_dir().context("could not find the home directory")?;
            let saved = store.index()?;
            let report = nori_providers::detect_all(&DetectContext {
                home: &home,
                env: &env,
                platform: Platform::current(),
                keychain: &SystemKeychain,
                saved: &saved,
            });
            if command.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_table(&report);
            }
        }
        Some(ProvidersAction::SetKey { agent, json }) => {
            let agent = parse_agent(&agent)?;
            let mut stdin = std::io::stdin();
            if stdin.is_terminal() {
                eprintln!(
                    "Paste the API key for {} and press Enter, then Ctrl-D:",
                    agent.display_name()
                );
            }
            let mut key = String::new();
            stdin.read_to_string(&mut key)?;
            let location = store.save(agent, &key)?;
            let env = nori_providers::saved_key_env(agent).unwrap_or_default();
            let store = match location {
                StoreLocation::Keyring => "keyring",
                StoreLocation::File => "file",
            };
            if json {
                println!(
                    "{}",
                    json!({"agent": agent.slug(), "env": env, "store": store})
                );
            } else {
                println!(
                    "Saved the {} API key ({store}). Nori passes it as {env} unless a credential variable for the agent is already set.",
                    agent.display_name()
                );
            }
        }
        Some(ProvidersAction::RemoveKey { agent }) => {
            let agent = parse_agent(&agent)?;
            let name = agent.display_name();
            if store.delete(agent)? {
                println!("Removed the saved {name} API key.");
            } else {
                println!("No saved {name} API key.");
            }
        }
    }
    Ok(())
}

fn print_table(report: &nori_providers::ProvidersReport) {
    let rows: Vec<[String; 4]> = report
        .agents
        .iter()
        .map(|agent| {
            let status = match agent.status {
                AuthStatus::SignedIn => "signed in",
                AuthStatus::SignedOut => "signed out",
                AuthStatus::Unknown => "unknown",
            };
            let (credential, source) = match agent.credentials.first() {
                Some(credential) => {
                    let kind = match credential.kind {
                        CredentialKind::Subscription => "subscription",
                        CredentialKind::ApiKey => "API key",
                        CredentialKind::Cloud => "cloud",
                        CredentialKind::Token => "token",
                        CredentialKind::Login => "login",
                    };
                    let kind = match &credential.detail {
                        Some(detail) => format!("{kind} ({detail})"),
                        None => kind.to_string(),
                    };
                    let source = match &credential.source {
                        CredentialSource::Env { name } => format!("${name}"),
                        CredentialSource::File { path } => path.display().to_string(),
                        CredentialSource::Keychain { service } => {
                            format!("keychain: {service}")
                        }
                        CredentialSource::NoriSaved { env, store } => match store {
                            StoreLocation::Keyring => format!("saved in keychain as ${env}"),
                            StoreLocation::File => format!("saved in file as ${env}"),
                        },
                    };
                    (kind, source)
                }
                None => ("-".to_string(), agent.hint.clone()),
            };
            [agent.agent.clone(), status.to_string(), credential, source]
        })
        .collect();
    let header = [
        "AGENT".to_string(),
        "STATUS".to_string(),
        "CREDENTIAL".to_string(),
        "SOURCE".to_string(),
    ];
    let widths: Vec<usize> = (0..3)
        .map(|column| {
            std::iter::once(&header)
                .chain(&rows)
                .map(|row| row[column].len())
                .max()
                .unwrap_or_default()
        })
        .collect();
    for row in std::iter::once(&header).chain(&rows) {
        println!(
            "{:w0$}  {:w1$}  {:w2$}  {}",
            row[0],
            row[1],
            row[2],
            row[3],
            w0 = widths[0],
            w1 = widths[1],
            w2 = widths[2]
        );
    }
}
