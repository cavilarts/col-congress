use std::net::SocketAddr;

pub const DEFAULT_BIND_ADDRESS: &str = "0.0.0.0:3000";
pub const DEFAULT_LOG_FILTER: &str = "info";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub database_url: String,
    pub bind_address: SocketAddr,
    pub app_token: Option<String>,
    pub log_filter: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("missing required environment variable {0}")]
    Missing(&'static str),
    #[error("invalid value for environment variable {0}")]
    Invalid(&'static str),
}

impl Config {
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Config, ConfigError> {
        let database_url =
            non_empty(lookup("DATABASE_URL")).ok_or(ConfigError::Missing("DATABASE_URL"))?;
        let bind_address = non_empty(lookup("BIND_ADDRESS"))
            .unwrap_or_else(|| DEFAULT_BIND_ADDRESS.to_string())
            .parse()
            .map_err(|_| ConfigError::Invalid("BIND_ADDRESS"))?;
        let app_token = non_empty(lookup("DATOS_GOV_CO_APP_TOKEN"));
        let log_filter =
            non_empty(lookup("RUST_LOG")).unwrap_or_else(|| DEFAULT_LOG_FILTER.to_string());
        Ok(Config {
            database_url,
            bind_address,
            app_token,
            log_filter,
        })
    }

    pub fn from_env() -> Result<Config, ConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn defaults_apply_when_optional_values_missing() {
        let config = Config::from_lookup(lookup(&[("DATABASE_URL", "postgres://x")])).unwrap();
        assert_eq!(config.bind_address.to_string(), "0.0.0.0:3000");
        assert_eq!(config.app_token, None);
        assert_eq!(config.log_filter, "info");
    }

    #[test]
    fn database_url_is_required() {
        assert_eq!(
            Config::from_lookup(lookup(&[])),
            Err(ConfigError::Missing("DATABASE_URL"))
        );
        assert_eq!(
            Config::from_lookup(lookup(&[("DATABASE_URL", "   ")])),
            Err(ConfigError::Missing("DATABASE_URL"))
        );
    }

    #[test]
    fn bad_bind_address_is_invalid() {
        assert_eq!(
            Config::from_lookup(lookup(&[("DATABASE_URL", "x"), ("BIND_ADDRESS", "nope")])),
            Err(ConfigError::Invalid("BIND_ADDRESS"))
        );
    }

    #[test]
    fn all_values_are_read() {
        let config = Config::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://x"),
            ("BIND_ADDRESS", "127.0.0.1:4000"),
            ("DATOS_GOV_CO_APP_TOKEN", "tok"),
            ("RUST_LOG", "debug"),
        ]))
        .unwrap();
        assert_eq!(config.bind_address.to_string(), "127.0.0.1:4000");
        assert_eq!(config.app_token.as_deref(), Some("tok"));
        assert_eq!(config.log_filter, "debug");
    }

    #[test]
    fn empty_optional_values_are_treated_as_unset() {
        let config = Config::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://x"),
            ("DATOS_GOV_CO_APP_TOKEN", "  "),
            ("BIND_ADDRESS", ""),
        ]))
        .unwrap();
        assert_eq!(config.app_token, None);
        assert_eq!(config.bind_address.to_string(), "0.0.0.0:3000");
    }

    #[test]
    fn from_env_reads_process_environment() {
        let result = Config::from_env();
        if std::env::var("DATABASE_URL").is_ok() {
            assert!(result.is_ok());
        } else {
            assert_eq!(result, Err(ConfigError::Missing("DATABASE_URL")));
        }
    }

    #[test]
    fn error_messages_name_the_variable() {
        assert!(
            ConfigError::Missing("DATABASE_URL")
                .to_string()
                .contains("DATABASE_URL")
        );
        assert!(
            ConfigError::Invalid("BIND_ADDRESS")
                .to_string()
                .contains("BIND_ADDRESS")
        );
    }
}
