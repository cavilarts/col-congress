use reqwest::Client;

use super::parser::parse_html;
use crate::adapters::outbound::datos_gov_co::client::{TIMEOUT, USER_AGENT};
use crate::application::errors::SourceError;
use crate::application::ports::SenatorSource;
use crate::domain::senator::{Senator, SourceKind, Term};

pub const SENADO_SENATORS_URL: &str =
    "https://www.senado.gov.co/index.php/el-senado/senadores?lastletter=Todos";

pub struct SenadoWebSource {
    http: Client,
    url: String,
}

impl SenadoWebSource {
    pub fn new(url: impl Into<String>) -> Result<Self, reqwest::Error> {
        let http = Client::builder()
            .timeout(TIMEOUT)
            .user_agent(USER_AGENT)
            .build()?;
        Ok(Self {
            http,
            url: url.into(),
        })
    }
}

impl SenatorSource for SenadoWebSource {
    fn kind(&self) -> SourceKind {
        SourceKind::Senado
    }

    async fn fetch(&self, term: &Term) -> Result<Vec<Senator>, SourceError> {
        let response = self
            .http
            .get(&self.url)
            .send()
            .await
            .map_err(|_| SourceError::Network)?;
        let status = response.status();
        if !status.is_success() {
            return Err(SourceError::UnexpectedStatus(status.as_u16()));
        }
        let html = response.text().await.map_err(|_| SourceError::Network)?;
        parse_html(&html, term)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::outbound::testing::{serve_once, unused_port_base};

    fn term() -> Term {
        Term::parse("2026-2030").unwrap()
    }

    const PAGE: &str = r#"<div class="modazdirectory__results"><div class="modazdirectory__result"><div><h3>Ana Paola</h3><p>MIRA</p></div></div></div>"#;

    #[tokio::test]
    async fn fetches_and_parses_page() {
        let (base, request) = serve_once(200, PAGE).await;
        let source = SenadoWebSource::new(format!("{base}/senadores?lastletter=Todos")).unwrap();
        let senators = source.fetch(&term()).await.unwrap();
        assert_eq!(senators.len(), 1);
        let raw = request.await.unwrap();
        assert!(raw.starts_with("GET /senadores?lastletter=Todos"), "{raw}");
    }

    #[tokio::test]
    async fn non_success_status_is_unexpected_status() {
        let (base, _request) = serve_once(500, "error").await;
        let source = SenadoWebSource::new(base).unwrap();
        assert_eq!(
            source.fetch(&term()).await,
            Err(SourceError::UnexpectedStatus(500))
        );
    }

    #[tokio::test]
    async fn page_without_container_is_parse_error() {
        let (base, _request) = serve_once(200, "<html></html>").await;
        let source = SenadoWebSource::new(base).unwrap();
        assert!(matches!(
            source.fetch(&term()).await,
            Err(SourceError::Parse(_))
        ));
    }

    #[tokio::test]
    async fn connection_refused_is_network_error() {
        let base = unused_port_base().await;
        let source = SenadoWebSource::new(base).unwrap();
        assert_eq!(source.fetch(&term()).await, Err(SourceError::Network));
    }

    #[test]
    fn kind_and_default_url() {
        let source = SenadoWebSource::new(SENADO_SENATORS_URL).unwrap();
        assert_eq!(source.kind(), SourceKind::Senado);
        assert!(SENADO_SENATORS_URL.contains("lastletter=Todos"));
    }
}
