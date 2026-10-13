use std::collections::HashSet;
use std::time::Duration;

use reqwest::Client;
use scraper::Html;

use super::parser::{parse_cards, parse_listing};
use crate::adapters::outbound::datos_gov_co::client::USER_AGENT;
use crate::application::errors::SourceError;
use crate::application::ports::RepresentativeSource;
use crate::domain::representative::Representative;
use crate::domain::senator::{SourceKind, Term};

pub const LISTING_URL: &str = "https://congresovisible.uniandes.edu.co/congresistas/";
pub const TIMEOUT: Duration = Duration::from_secs(30);
pub const PAGE_SIZE: &str = "500";
pub const CAMARA_CORPORATION: &str = "1";
pub const TERM_CUATRIENIO: &str = "10";
const MAX_PAGES: u32 = 10;

pub struct CongresoVisibleSource {
    http: Client,
    listing_url: String,
}

impl CongresoVisibleSource {
    pub fn new(listing_url: impl Into<String>) -> Result<Self, reqwest::Error> {
        let http = Client::builder()
            .timeout(TIMEOUT)
            .user_agent(USER_AGENT)
            .build()?;
        Ok(Self {
            http,
            listing_url: listing_url.into(),
        })
    }

    async fn fetch_page(&self, page: u32) -> Result<String, SourceError> {
        let page = page.to_string();
        let response = self
            .http
            .get(&self.listing_url)
            .query(&[
                ("page", page.as_str()),
                ("rows", PAGE_SIZE),
                ("search", ""),
                ("corporacion", CAMARA_CORPORATION),
                ("cuatrienio", TERM_CUATRIENIO),
            ])
            .send()
            .await
            .map_err(|_| SourceError::Network)?;
        let status = response.status();
        if !status.is_success() {
            return Err(SourceError::UnexpectedStatus(status.as_u16()));
        }
        response.text().await.map_err(|_| SourceError::Network)
    }
}

impl RepresentativeSource for CongresoVisibleSource {
    fn kind(&self) -> SourceKind {
        SourceKind::CongresoVisible
    }

    async fn fetch(&self, term: &Term) -> Result<Vec<Representative>, SourceError> {
        let mut all = Vec::new();
        let mut seen = HashSet::new();
        for page in 1..=MAX_PAGES {
            let html = self.fetch_page(page).await?;
            let cards = if page == 1 {
                parse_listing(&html, term)?
            } else {
                parse_cards(&Html::parse_document(&html), term)
            };
            let new: Vec<Representative> = cards
                .into_iter()
                .filter(|r| seen.insert(r.normalized_name.clone()))
                .collect();
            if page > 1 && new.is_empty() {
                break;
            }
            all.extend(new);
        }
        Ok(all)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::outbound::testing::{serve_once, serve_sequence, unused_port_base};

    const CARDS: &str = r#"<div class="integrantesContainer"><a href="/congresistas/perfil/a/1"><ul><li><strong>Ana Paola</strong></li><li>MIRA</li></ul></a></div>"#;

    fn term() -> Term {
        Term::parse("2026-2030").unwrap()
    }

    #[tokio::test]
    async fn fetches_listing_with_expected_query() {
        let (base, requests) = serve_sequence(vec![
            (200, CARDS),
            (200, r#"<div class="integrantesContainer"></div>"#),
        ])
        .await;
        let source = CongresoVisibleSource::new(format!("{base}/congresistas/")).unwrap();
        let reps = RepresentativeSource::fetch(&source, &term()).await.unwrap();
        assert_eq!(reps.len(), 1);
        let raw = requests.await.unwrap().remove(0);
        assert!(raw.starts_with("GET /congresistas/?"), "{raw}");
        assert!(raw.contains("rows=500"), "{raw}");
        assert!(raw.contains("corporacion=1"), "{raw}");
        assert!(raw.contains("cuatrienio=10"), "{raw}");
        assert!(raw.contains("page=1"), "{raw}");
    }

    #[tokio::test]
    async fn non_success_status_is_unexpected_status() {
        let (base, _request) = serve_once(503, "down").await;
        let source = CongresoVisibleSource::new(format!("{base}/congresistas/")).unwrap();
        assert_eq!(
            RepresentativeSource::fetch(&source, &term()).await,
            Err(SourceError::UnexpectedStatus(503))
        );
    }

    #[tokio::test]
    async fn page_without_container_is_parse_error() {
        let (base, _request) = serve_once(200, "<html></html>").await;
        let source = CongresoVisibleSource::new(format!("{base}/congresistas/")).unwrap();
        assert!(matches!(
            RepresentativeSource::fetch(&source, &term()).await,
            Err(SourceError::Parse(_))
        ));
    }

    #[tokio::test]
    async fn paging_stops_on_an_empty_page() {
        let (base, requests) = serve_sequence(vec![
            (200, CARDS),
            (200, r#"<div class="integrantesContainer"></div>"#),
        ])
        .await;
        let source = CongresoVisibleSource::new(format!("{base}/congresistas/")).unwrap();
        let reps = RepresentativeSource::fetch(&source, &term()).await.unwrap();
        assert_eq!(reps.len(), 1);
        let raw = requests.await.unwrap();
        assert_eq!(
            raw.len(),
            2,
            "paging stops at the first page without new cards"
        );
    }

    #[tokio::test]
    async fn connection_refused_is_network_error() {
        let base = unused_port_base().await;
        let source = CongresoVisibleSource::new(format!("{base}/congresistas/")).unwrap();
        assert_eq!(
            RepresentativeSource::fetch(&source, &term()).await,
            Err(SourceError::Network)
        );
    }

    #[test]
    fn kind_and_listing_url() {
        let source = CongresoVisibleSource::new(LISTING_URL).unwrap();
        assert_eq!(
            RepresentativeSource::kind(&source),
            SourceKind::CongresoVisible
        );
        assert!(LISTING_URL.ends_with("/congresistas/"));
    }

    #[tokio::test]
    #[ignore = "calls the live Congreso Visible site; run with --ignored"]
    async fn live_listing_returns_the_current_term() {
        let source = CongresoVisibleSource::new(LISTING_URL).unwrap();
        let reps = RepresentativeSource::fetch(&source, &term()).await.unwrap();
        assert!(reps.len() >= 150, "got {}", reps.len());
        assert!(reps.iter().all(|r| r.source == SourceKind::CongresoVisible));
        assert!(reps.iter().all(|r| r.constituency.is_none()));
    }
}
