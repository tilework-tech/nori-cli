//! `nori auth`: per-agent sign-in status, API keys saved through Nori, and the
//! per-agent choice between the agent's own login and a saved key.

mod key_input;
mod prefer;
mod table;

use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use codex_keyring_store::DefaultKeyringStore;
use nori_auth_providers::AgentKind;
use nori_auth_providers::AuthReport;
use nori_auth_providers::Credential;
use nori_auth_providers::CredentialStore;
use nori_auth_providers::DetectContext;
use nori_auth_providers::Platform;
use nori_auth_providers::Provider;
use nori_auth_providers::SavedIndex;
use nori_auth_providers::StoreBackend;
use nori_auth_providers::SystemKeychain;
use nori_config::NoriConfig;
use serde_json::json;

#[derive(Debug, Parser)]
pub(crate) struct AuthCommand {
    #[command(subcommand)]
    action: Option<AuthAction>,
    /// Print the full report as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, clap::Subcommand)]
enum AuthAction {
    /// Save an API key for a model provider. At a terminal the key is read
    /// hidden; otherwise it is read from stdin. Agents receive it only after
    /// `nori auth prefer-api-key <agent>`.
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
    /// Pass saved API keys to an agent at launch instead of letting it use its
    /// own login. Asks once when the agent is signed in with a subscription,
    /// because usage is then billed to the API account.
    PreferApiKey {
        /// Agent slug, e.g. claude-code or codex.
        agent: String,
        /// Accept the billing change without asking.
        #[arg(long)]
        yes: bool,
        /// Print `{"agent", "prefer_api_key", "api_billing_confirmed"}`, or
        /// `{"error": "confirmation_required", "code", "message"}`.
        #[arg(long)]
        json: bool,
    },
    /// Let an agent use its own login again (the default).
    PreferLogin {
        /// Agent slug, e.g. claude-code or codex.
        agent: String,
        /// Print `{"agent", "prefer_api_key", "api_billing_confirmed"}`.
        #[arg(long)]
        json: bool,
    },
}

pub(crate) fn run(command: AuthCommand) -> anyhow::Result<()> {
    let env = |key: &str| std::env::var(key).ok();
    let nori_home = nori_config::find_nori_home()?;
    let store = CredentialStore::new(
        &nori_home,
        StoreBackend::from_env(&env)?,
        Arc::new(DefaultKeyringStore),
    );
    match command.action {
        None => {
            let report = report(&nori_home, &store)?;
            if command.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                table::print_table(&report);
            }
        }
        Some(AuthAction::SetKey { provider, json }) => {
            let provider = Provider::parse(&provider)?;
            let key = key_input::read_key(provider.display_name())?;
            let credential = Credential::api_key(&key)?;
            let key = key.trim();
            let (prefix, issuer) = nori_auth_providers::key_prefix(key).unwrap_or(("", ""));
            if !issuer.is_empty() && !provider.display_name().starts_with(issuer) {
                let article = if issuer.starts_with(['A', 'E', 'I', 'O', 'U', 'x']) {
                    "an"
                } else {
                    "a"
                };
                eprintln!(
                    "warning: this key looks like {article} {issuer} key, not {} (saved anyway)",
                    provider.display_name()
                );
            }
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
                    "Saved the {} API key {prefix}**** ({store_name}). Agents that prefer API keys \
                     receive it: {}. Run `nori auth prefer-api-key <agent>` to choose one.",
                    provider.display_name(),
                    agents.join(", ")
                );
            }
        }
        Some(AuthAction::RemoveKey { provider }) => {
            let provider = Provider::parse(&provider)?;
            let name = provider.display_name();
            if store.remove(provider)? {
                println!("Removed the saved {name} credential.");
            } else {
                println!("No saved {name} credential.");
            }
        }
        Some(AuthAction::PreferApiKey { agent, yes, json }) => {
            let agent = prefer::parse_agent(&agent)?;
            let config = load_config(&nori_home)?;
            let home = dirs::home_dir().context("could not find the home directory")?;
            let prefer = config.auth_preference(agent.slug()).prefer_api_key;
            let local = nori_auth_providers::detect_local_agent(agent, &home, &nori_home, prefer)?;
            prefer::prefer_api_key(&nori_home, &config, &local, yes, json)?;
        }
        Some(AuthAction::PreferLogin { agent, json }) => {
            let agent = prefer::parse_agent(&agent)?;
            let config = load_config(&nori_home)?;
            prefer::prefer_login(&nori_home, &config, agent, json)?;
        }
    }
    Ok(())
}

fn load_config(nori_home: &Path) -> anyhow::Result<NoriConfig> {
    NoriConfig::load_from_path(&nori_home.join(nori_config::CONFIG_FILE))
}

fn report(nori_home: &Path, store: &CredentialStore) -> anyhow::Result<AuthReport> {
    let config = load_config(nori_home)?;
    let home = dirs::home_dir().context("could not find the home directory")?;
    // A broken store must not hide what detection can still see.
    let (saved, store_error) = match store.index() {
        Ok(saved) => (saved, None),
        Err(error) => (SavedIndex::default(), Some(format!("{error:#}"))),
    };
    let env = |key: &str| std::env::var(key).ok();
    let prefer = |agent: AgentKind| config.auth_preference(agent.slug()).prefer_api_key;
    let mut report = nori_auth_providers::detect_all(&DetectContext {
        home: &home,
        env: &env,
        platform: Platform::current(),
        keychain: &SystemKeychain,
        saved: &saved,
        prefer_api_key: &prefer,
    });
    report.store = store.backend();
    report.store_error = store_error;
    Ok(report)
}
