use nori_auth_providers::AuthStatus;
use nori_auth_providers::CredentialKind;
use nori_auth_providers::CredentialSource;

pub(super) fn print_table(report: &nori_auth_providers::AuthReport) {
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
