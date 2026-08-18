use crate::config::CONFIG_TOML_FILE;
use std::io;
use std::path::Path;
use tokio::fs;
use toml::Value as TomlValue;

/// Load the user-owned `config.toml`, or an empty table when it does not exist.
pub async fn load_config_as_toml(codex_home: &Path) -> io::Result<TomlValue> {
    let path = codex_home.join(CONFIG_TOML_FILE);
    match fs::read_to_string(&path).await {
        Ok(contents) => toml::from_str(&contents).map_err(|err| {
            tracing::error!("Failed to parse {}: {err}", path.display());
            io::Error::new(io::ErrorKind::InvalidData, err)
        }),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            tracing::info!("{} not found, using defaults", path.display());
            Ok(TomlValue::Table(Default::default()))
        }
        Err(err) => {
            tracing::error!("Failed to read {}: {err}", path.display());
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn missing_config_is_an_empty_table() {
        let tmp = tempdir().expect("tempdir");
        let loaded = load_config_as_toml(tmp.path()).await.expect("load config");
        assert!(loaded.as_table().is_some_and(toml::map::Map::is_empty));
    }

    #[tokio::test]
    async fn loads_user_config() {
        let tmp = tempdir().expect("tempdir");
        std::fs::write(tmp.path().join(CONFIG_TOML_FILE), "model = \"test\"\n")
            .expect("write config");

        let loaded = load_config_as_toml(tmp.path()).await.expect("load config");
        assert_eq!(
            loaded.get("model").and_then(TomlValue::as_str),
            Some("test")
        );
    }
}
