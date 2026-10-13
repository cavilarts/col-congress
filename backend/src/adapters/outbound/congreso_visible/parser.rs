use std::collections::HashSet;

use scraper::{ElementRef, Html, Selector};
use tracing::{info, warn};

use crate::application::errors::SourceError;
use crate::domain::normalize::clean_text;
use crate::domain::representative::Representative;
use crate::domain::senator::{SourceKind, Term};

const CONTAINER: &str = "div.integrantesContainer";
const CARD: &str = "a[href^='/congresistas/perfil/']";

pub fn parse_listing(html: &str, term: &Term) -> Result<Vec<Representative>, SourceError> {
    let document = Html::parse_document(html);
    let container = Selector::parse(CONTAINER).expect("static selector is valid");
    if document.select(&container).next().is_none() {
        return Err(SourceError::Parse(
            "representatives listing container not found".into(),
        ));
    }
    let cards = parse_cards(&document, term);
    if cards.is_empty() {
        return Err(SourceError::Empty);
    }
    Ok(cards)
}

pub fn parse_cards(document: &Html, term: &Term) -> Vec<Representative> {
    let card_sel = Selector::parse(CARD).expect("static selector is valid");
    let li_sel = Selector::parse("li").expect("static selector is valid");
    let mut seen = HashSet::new();
    let mut representatives = Vec::new();
    let mut total = 0;
    let mut skipped = 0;
    let mut duplicates = 0;

    for card in document.select(&card_sel) {
        total += 1;
        let items: Vec<ElementRef<'_>> = card.select(&li_sel).collect();
        let Some(name) = items.first().and_then(|li| clean_text(&element_text(*li))) else {
            skipped += 1;
            warn!(
                position = total,
                "congreso visible card without a name skipped"
            );
            continue;
        };
        let mut representative =
            Representative::new(term.clone(), name, SourceKind::CongresoVisible);
        if !seen.insert(representative.normalized_name.clone()) {
            duplicates += 1;
            continue;
        }
        representative.party = items
            .get(1)
            .and_then(|li| clean_text(&element_text(*li)))
            .filter(|text| !text.to_lowercase().starts_with("comisión"));
        representatives.push(representative);
    }

    info!(
        cards = total,
        records = representatives.len(),
        skipped,
        duplicates,
        "congreso visible cards parsed"
    );
    representatives
}

fn element_text(element: ElementRef<'_>) -> String {
    element.text().collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str =
        include_str!("../../../../tests/fixtures/congreso_visible_representatives.html");

    fn term() -> Term {
        Term::parse("2026-2030").unwrap()
    }

    #[test]
    fn fixture_yields_unique_representatives() {
        let reps = parse_listing(FIXTURE, &term()).unwrap();
        assert_eq!(reps.len(), 3);
        assert_eq!(reps[0].full_name, "Abasolo Gómez Alejandra Gabriela");
        assert_eq!(reps[0].party.as_deref(), Some("Avancemos Nariño"));
        assert_eq!(reps[0].source, SourceKind::CongresoVisible);
        assert_eq!(reps[0].constituency, None);
        assert_eq!(
            reps[0].id.as_str(),
            "2026-2030-abasolo-gomez-alejandra-gabriela"
        );
    }

    #[test]
    fn fixture_empty_party_is_none() {
        let reps = parse_listing(FIXTURE, &term()).unwrap();
        assert!(reps.iter().any(|r| r.party.is_none()));
    }

    #[test]
    fn commission_text_is_not_a_party() {
        let html = r#"<div class="integrantesContainer"><a href="/congresistas/perfil/x/1"><ul><li><strong>Ana Paola</strong></li><li>Comisión: Sin comisión</li></ul></a></div>"#;
        let reps = parse_listing(html, &term()).unwrap();
        assert_eq!(reps[0].party, None);
    }

    #[test]
    fn missing_container_is_parse_error() {
        assert!(matches!(
            parse_listing("<html></html>", &term()),
            Err(SourceError::Parse(_))
        ));
    }

    #[test]
    fn container_without_cards_is_empty() {
        assert_eq!(
            parse_listing(r#"<div class="integrantesContainer"></div>"#, &term()),
            Err(SourceError::Empty)
        );
    }

    #[test]
    fn card_without_name_is_skipped_and_counted() {
        let html = r#"<div class="integrantesContainer">
            <a href="/congresistas/perfil/a/1"><ul><li>   </li><li>Partido</li></ul></a>
            <a href="/congresistas/perfil/b/2"><ul><li><strong>Luis Pérez</strong></li><li>MIRA</li></ul></a>
        </div>"#;
        let reps = parse_listing(html, &term()).unwrap();
        assert_eq!(reps.len(), 1);
        assert_eq!(reps[0].full_name, "Luis Pérez");
    }

    #[test]
    fn duplicates_are_removed_first_kept() {
        let html = r#"<div class="integrantesContainer">
            <a href="/congresistas/perfil/a/1"><ul><li><strong>Luis Pérez</strong></li><li>A</li></ul></a>
            <a href="/congresistas/perfil/b/2"><ul><li><strong>LUIS  PÉREZ</strong></li><li>B</li></ul></a>
        </div>"#;
        let reps = parse_listing(html, &term()).unwrap();
        assert_eq!(reps.len(), 1);
        assert_eq!(reps[0].party.as_deref(), Some("A"));
    }
}
