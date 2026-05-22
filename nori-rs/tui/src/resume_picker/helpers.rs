use super::*;

pub(super) fn rows_from_items(items: Vec<SessionMetadata>, nori_home: PathBuf) -> Vec<Row> {
    items
        .into_iter()
        .map(|item| metadata_to_row(item, nori_home.clone()))
        .collect()
}

fn metadata_to_row(item: SessionMetadata, nori_home: PathBuf) -> Row {
    let created_at = parse_timestamp_str(&item.started_at);
    let updated_at = created_at;
    let cwd = Some(item.cwd.clone());
    let preview = item.session_id.clone();

    Row {
        target: ResumeTarget {
            nori_home,
            project_id: item.project_id,
            session_id: item.session_id,
            agent: item.agent,
        },
        preview,
        created_at,
        updated_at,
        cwd,
        git_branch: None,
    }
}

pub(super) fn paths_match(a: &Path, b: &Path) -> bool {
    if let (Ok(ca), Ok(cb)) = (a.canonicalize(), b.canonicalize()) {
        return ca == cb;
    }
    a == b
}

fn parse_timestamp_str(ts: &str) -> Option<DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(ts)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
}

pub(super) fn human_time_ago(ts: DateTime<Utc>) -> String {
    let now = Utc::now();
    let delta = now - ts;
    let secs = delta.num_seconds();
    if secs < 60 {
        let n = secs.max(0);
        if n == 1 {
            format!("{n} second ago")
        } else {
            format!("{n} seconds ago")
        }
    } else if secs < 60 * 60 {
        let m = secs / 60;
        if m == 1 {
            format!("{m} minute ago")
        } else {
            format!("{m} minutes ago")
        }
    } else if secs < 60 * 60 * 24 {
        let h = secs / 3600;
        if h == 1 {
            format!("{h} hour ago")
        } else {
            format!("{h} hours ago")
        }
    } else {
        let d = secs / (60 * 60 * 24);
        if d == 1 {
            format!("{d} day ago")
        } else {
            format!("{d} days ago")
        }
    }
}

pub(super) fn format_updated_label(row: &Row) -> String {
    match (row.updated_at, row.created_at) {
        (Some(updated), _) => human_time_ago(updated),
        (None, Some(created)) => human_time_ago(created),
        (None, None) => "-".to_string(),
    }
}
