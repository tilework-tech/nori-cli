//! `nori providers`: per-agent sign-in status and API keys saved through Nori.

use std::io::IsTerminal;
use std::io::Read;
use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use codex_keyring_store::DefaultKeyringStore;
use nori_providers::AuthStatus;
use nori_providers::Credential;
use nori_providers::CredentialKind;
use nori_providers::CredentialSource;
use nori_providers::CredentialStore;
use nori_providers::DetectContext;
use nori_providers::Platform;
use nori_providers::Provider;
use nori_providers::StoreBackend;
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
    /// Save an API key for a model provider, read from stdin. Every agent that
    /// talks to the provider receives it at launch, in its own variable, unless
    /// that agent already has a credential for the provider in its environment.
    SetKey {
        /// Provider: anthropic, openai, google, xai, or cursor.
        provider: String,
        /// Print `{"provider", "type", "store", "agents"}`.
        #[arg(long)]
        json: bool,
    },
    /// Forget the key saved for a provider.
    RemoveKey {
        /// Provider whose saved key to remove.
        provider: String,
    },
}

pub(crate) fn run(command: ProvidersCommand) -> anyhow::Result<()> {
    let env = |key: &str| std::env::var(key).ok();
    let nori_home = nori_config::find_nori_home()?;
    let store = CredentialStore::new(
        &nori_home,
        StoreBackend::from_env(&env)?,
        Arc::new(DefaultKeyringStore),
    );
    match command.action {
        None => {
            let home = dirs::home_dir().context("could not find the home directory")?;
            // A broken store must not hide what detection can still see.
            let (saved, store_error) = match store.index() {
                Ok(saved) => (saved, None),
                Err(error) => (
                    nori_providers::SavedIndex::default(),
                    Some(format!("{error:#}")),
                ),
            };
            let mut report = nori_providers::detect_all(&DetectContext {
                home: &home,
                env: &env,
                platform: Platform::current(),
                keychain: &SystemKeychain,
                saved: &saved,
            });
            report.store = store.backend();
            report.store_error = store_error;
            if command.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_table(&report);
            }
        }
        Some(ProvidersAction::SetKey { provider, json }) => {
            let provider = Provider::parse(&provider)?;
            let mut stdin = std::io::stdin();
            if stdin.is_terminal() {
                eprintln!(
                    "Paste the {} API key and press Enter, then Ctrl-D:",
                    provider.display_name()
                );
            }
            let mut key = String::new();
            stdin.read_to_string(&mut key)?;
            let credential = Credential::api_key(&key)?;
            let record_type = credential.type_name();
            store.set(provider, credential)?;
            let routes: Vec<_> = provider
                .agent_routes()
                .into_iter()
                .map(|(agent, env)| (agent.slug(), env))
                .collect();
            let store_name = store.backend().as_str();
            if json {
                let agents: Vec<_> = routes
                    .iter()
                    .map(|(agent, env)| json!({"agent": agent, "env": env}))
                    .collect();
                println!(
                    "{}",
                    json!({"provider": provider.id(), "type": record_type, "store": store_name, "agents": agents})
                );
            } else {
                let agents: Vec<_> = routes
                    .iter()
                    .map(|(agent, env)| format!("{agent} (${env})"))
                    .collect();
                println!(
                    "Saved the {} API key ({store_name}). Nori passes it to {} unless the agent already has a credential for {} in its environment.",
                    provider.display_name(),
                    agents.join(", "),
                    provider.id()
                );
            }
        }
        Some(ProvidersAction::RemoveKey { provider }) => {
            let provider = Provider::parse(&provider)?;
            let name = provider.display_name();
            if store.remove(provider)? {
                println!("Removed the saved {name} credential.");
            } else {
                println!("No saved {name} credential.");
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
                    let label = [credential.provider.as_deref(), credential.detail.as_deref()]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(", ");
                    let kind = if label.is_empty() {
                        kind.to_string()
                    } else {
                        format!("{kind} ({label})")
                    };
                    let source = match &credential.source {
                        CredentialSource::Env { name } => format!("${name}"),
                        CredentialSource::File { path } => path.display().to_string(),
                        CredentialSource::Keychain { service } => {
                            format!("keychain: {service}")
                        }
                        CredentialSource::NoriSaved { env, .. } => {
                            format!("saved through Nori, as ${env}")
                        }
                    };
                    let source = if credential.read_only {
                        format!("{source}; read-only: {}", credential.disconnect)
                    } else {
                        source
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
    for agent in &report.agents {
        for notice in &agent.notices {
            println!("note: {}", notice.message);
        }
    }
    let saved: Vec<_> = report
        .providers
        .iter()
        .filter(|provider| provider.saved.is_some())
        .map(|provider| provider.id.id())
        .collect();
    if !saved.is_empty() {
        println!(
            "saved keys ({}): {}",
            report.store.as_str(),
            saved.join(", ")
        );
    }
    if let Some(error) = &report.store_error {
        eprintln!("warning: saved keys are unavailable: {error}");
    }
}
