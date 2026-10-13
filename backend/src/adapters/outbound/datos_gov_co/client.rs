use std::time::Duration;

use reqwest::Client;

use super::mapping::parse_body;
use super::representatives_mapping::parse_representatives_body;
use crate::application::errors::SourceError;
use crate::application::ports::{RepresentativeSource, SenatorSource};
use crate::domain::representative::Representative;
use crate::domain::senator::{Senator, SourceKind, Term};

pub const USER_AGENT: &str = concat!("congreso-backend/", env!("CARGO_PKG_VERSION"));
pub const TIMEOUT: Duration = Duration::from_secs(30);
pub const SENATORS_DATASET: &str = "sjwx-dr6n";
pub const PAGE_LIMIT: &str = "1000";

pub struct DatosGovCoSource {
    http: Client,
    base_url: String,
    dataset: String,
    app_token: Option<String>,
}

impl DatosGovCoSource {
    pub fn new(
        base_url: impl Into<String>,
        dataset: impl Into<String>,
        app_token: Option<String>,
    ) -> Result<Self, reqwest::Error> {
        let http = Client::builder()
            .timeout(TIMEOUT)
            .user_agent(USER_AGENT)
            .build()?;
        Ok(Self {
            http,
            base_url: base_url.into(),
            dataset: dataset.into(),
            app_token,
        })
    }

    fn resource_url(&self) -> String {
        format!(
            "{}/resource/{}.json",
            self.base_url.trim_end_matches('/'),
            self.dataset
        )
    }
}

impl DatosGovCoSource {
    async fn fetch_body(&self, where_clause: Option<String>) -> Result<String, SourceError> {
        let mut query = vec![("$limit", PAGE_LIMIT.to_string())];
        if let Some(clause) = where_clause {
            query.push(("$where", clause));
        }
        let mut request = self.http.get(self.resource_url()).query(&query);
        if let Some(token) = &self.app_token {
            request = request.header("X-App-Token", token);
        }

        let response = request.send().await.map_err(|_| SourceError::Network)?;
        let status = response.status();
        if !status.is_success() {
            return Err(SourceError::UnexpectedStatus(status.as_u16()));
        }
        response.text().await.map_err(|_| SourceError::Network)
    }
}

impl SenatorSource for DatosGovCoSource {
    fn kind(&self) -> SourceKind {
        SourceKind::DatosGovCo
    }

    async fn fetch(&self, term: &Term) -> Result<Vec<Senator>, SourceError> {
        let body = self
            .fetch_body(Some(format!("periodo='{}'", term.as_str())))
            .await?;
        parse_body(&body, term)
    }
}

impl RepresentativeSource for DatosGovCoSource {
    fn kind(&self) -> SourceKind {
        SourceKind::DatosGovCo
    }

    async fn fetch(&self, term: &Term) -> Result<Vec<Representative>, SourceError> {
        let body = self.fetch_body(None).await?;
        parse_representatives_body(&body, term)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::outbound::testing::{serve_once, unused_port_base};

    fn term() -> Term {
        Term::parse("2018-2022").unwrap()
    }

    #[tokio::test]
    async fn success_returns_mapped_senators_and_sends_token() {
        let (base, request) = serve_once(200, r#"[{"nombre":"Ana Paola","partido":"MIRA"}]"#).await;
        let source =
            DatosGovCoSource::new(base, SENATORS_DATASET, Some("secret-token".into())).unwrap();
        let senators = SenatorSource::fetch(&source, &term()).await.unwrap();
        assert_eq!(senators.len(), 1);
        assert_eq!(senators[0].party.as_deref(), Some("MIRA"));

        let raw = request.await.unwrap();
        assert!(raw.starts_with("GET /resource/sjwx-dr6n.json?"), "{raw}");
        assert!(
            raw.contains("%24limit=1000") || raw.contains("$limit=1000"),
            "{raw}"
        );
        assert!(
            raw.to_lowercase().contains("x-app-token: secret-token"),
            "{raw}"
        );
        assert!(
            raw.to_lowercase()
                .contains(&format!("user-agent: {USER_AGENT}").to_lowercase()),
            "{raw}"
        );
    }

    #[tokio::test]
    async fn no_token_header_when_unset() {
        let (base, request) = serve_once(200, "[]").await;
        let source = DatosGovCoSource::new(base, SENATORS_DATASET, None).unwrap();
        let senators = SenatorSource::fetch(&source, &term()).await.unwrap();
        assert!(senators.is_empty());
        let raw = request.await.unwrap();
        assert!(!raw.to_lowercase().contains("x-app-token"));
    }

    #[tokio::test]
    async fn non_success_status_is_unexpected_status() {
        let (base, _request) = serve_once(503, "down").await;
        let source = DatosGovCoSource::new(base, SENATORS_DATASET, None).unwrap();
        assert_eq!(
            SenatorSource::fetch(&source, &term()).await,
            Err(SourceError::UnexpectedStatus(503))
        );
    }

    #[tokio::test]
    async fn invalid_body_is_parse_error() {
        let (base, _request) = serve_once(200, "<html>not json</html>").await;
        let source = DatosGovCoSource::new(base, SENATORS_DATASET, None).unwrap();
        assert!(matches!(
            SenatorSource::fetch(&source, &term()).await,
            Err(SourceError::Parse(_))
        ));
    }

    #[tokio::test]
    async fn connection_refused_is_network_error() {
        let base = unused_port_base().await;
        let source = DatosGovCoSource::new(base, SENATORS_DATASET, None).unwrap();
        assert_eq!(
            SenatorSource::fetch(&source, &term()).await,
            Err(SourceError::Network)
        );
    }

    #[test]
    fn kind_is_datos_gov_co() {
        let source = DatosGovCoSource::new("http://localhost", SENATORS_DATASET, None).unwrap();
        assert_eq!(SenatorSource::kind(&source), SourceKind::DatosGovCo);
        assert_eq!(
            source.resource_url(),
            "http://localhost/resource/sjwx-dr6n.json"
        );
    }
}
