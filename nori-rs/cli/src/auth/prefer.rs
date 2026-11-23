//! `nori auth prefer-api-key` / `prefer-login`: the per-agent choice between
//! the agent's own login and a saved API key, kept in config.toml.

use std::io::BufRead;
use std::io::IsTerminal;
use std::io::Write;
use std::path::Path;

use crossterm::style::Stylize;
use nori_config::NoriConfig;
use nori_config::NoriConfigEdits;
use nori_providers::AgentKind;
use nori_providers::LocalAgentAuth;
use nori_providers::Provider;
use serde_json::json;

/// Parse an agent slug, pointing provider names at the agents that use them.
/// Agent names win, since `cursor` is both an agent and a provider.
pub(super) fn parse_agent(name: &str) -> anyhow::Result<AgentKind> {
    if let Some(agent) = AgentKind::from_slug(name) {
        if nori_providers::key_routes(agent).is_empty() {
            anyhow::bail!(
                "{} signs in through its own login only; it takes no saved API key",
                agent.display_name()
            );
        }
        return Ok(agent);
    }
    if let Some(provider) = Provider::from_id(name) {
        let agents: Vec<_> = provider
            .agent_routes()
            .into_iter()
            .map(|(agent, _)| agent.slug())
            .collect();
        anyhow::bail!(
            "{name:?} is a provider; choose an agent that uses it: {}",
            agents.join(", ")
        );
    }
    let accepting: Vec<_> = AgentKind::all()
        .iter()
        .filter(|agent| !nori_providers::key_routes(**agent).is_empty())
        .map(AgentKind::slug)
        .collect();
    anyhow::bail!("unknown agent; use one of {}", accepting.join(", "))
}

/// Set `agent` to prefer saved API keys. When it is signed in with a
/// subscription and the user has not accepted the billing change before, ask
/// at a terminal, or fail with `confirmation_required` unless `yes` is set.
pub(super) fn prefer_api_key(
    nori_home: &Path,
    config: &NoriConfig,
    local: &LocalAgentAuth,
    yes: bool,
    json: bool,
) -> anyhow::Result<()> {
    let status = &local.status;
    let agent = status.agent.as_str();
    let mut confirmed = config.auth_preference(agent).api_billing_confirmed;
    if let Some(warning) = status.api_key_billing_warning().filter(|_| !confirmed) {
        if !yes {
            if json || !std::io::stdin().is_terminal() {
                if json {
                    println!(
                        "{}",
                        json!({"error": "confirmation_required", "code": warning.code, "message": warning.message})
                    );
                }
                anyhow::bail!("{} Rerun with --yes to confirm.", warning.message);
            }
            if !confirm_billing(&warning.message)? {
                println!("Kept {} on its own login.", status.name);
                return Ok(());
            }
        }
        confirmed = true;
    }
    let mut edits =
        NoriConfigEdits::new(nori_home).set_path(&["auth", agent, "prefer_api_key"], true);
    if confirmed {
        edits = edits.set_path(&["auth", agent, "api_billing_confirmed"], true);
    }
    edits.apply_blocking()?;
    if json {
        println!(
            "{}",
            json!({"agent": agent, "prefer_api_key": true, "api_billing_confirmed": confirmed})
        );
        return Ok(());
    }
    println!(
        "{} prefers the saved API key from its next launch.",
        status.name
    );
    if local.saved_providers.is_empty() {
        let providers: Vec<_> = status
            .saved_keys
            .iter()
            .map(|route| route.provider.id())
            .collect();
        println!(
            "No key is saved for {} yet; run `nori auth set-key <provider>` to add one.",
            providers.join(" or ")
        );
    }
    Ok(())
}

pub(super) fn prefer_login(
    nori_home: &Path,
    config: &NoriConfig,
    agent: AgentKind,
    json: bool,
) -> anyhow::Result<()> {
    let slug = agent.slug();
    NoriConfigEdits::new(nori_home)
        .set_path(&["auth", slug, "prefer_api_key"], false)
        .apply_blocking()?;
    if json {
        let confirmed = config.auth_preference(slug).api_billing_confirmed;
        println!(
            "{}",
            json!({"agent": slug, "prefer_api_key": false, "api_billing_confirmed": confirmed})
        );
    } else {
        println!(
            "{} uses its own login from its next launch.",
            agent.display_name()
        );
    }
    Ok(())
}

/// Show the billing warning prominently and ask for an explicit `yes`.
fn confirm_billing(message: &str) -> anyhow::Result<bool> {
    let mut stderr = std::io::stderr();
    writeln!(stderr)?;
    writeln!(
        stderr,
        "{}",
        "  WARNING: THIS CHANGES WHO YOU ARE BILLED  "
            .bold()
            .white()
            .on_red()
    )?;
    writeln!(stderr, "{}", message.bold().red())?;
    write!(stderr, "Type {} to continue: ", "yes".bold())?;
    stderr.flush()?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    Ok(answer.trim().eq_ignore_ascii_case("yes"))
}
