use std::collections::HashSet;

use scraper::{ElementRef, Html, Selector};
use tracing::{info, warn};

use crate::application::errors::SourceError;
use crate::domain::normalize::clean_text;
use crate::domain::senator::{Senator, SourceKind, Term};

const CONTAINER: &str = "div.modazdirectory__results";
const CARD: &str = "div.modazdirectory__result";
const PHONE_LABEL: &str = "span.modazdirectory__label-phone";

pub fn parse_html(html: &str, term: &Term) -> Result<Vec<Senator>, SourceError> {
    let document = Html::parse_document(html);
    let container_sel = selector(CONTAINER);
    if document.select(&container_sel).next().is_none() {
        return Err(SourceError::Parse("senators container not found".into()));
    }

    let card_sel = selector(CARD);
    let mut seen = HashSet::new();
    let mut senators = Vec::new();
    let mut total = 0;
    let mut skipped = 0;
    let mut duplicates = 0;

    for card in document.select(&card_sel) {
        total += 1;
        let Some(name) = card_name(card) else {
            skipped += 1;
            warn!(position = total, "senado card without a name skipped");
            continue;
        };
        let mut senator = Senator::new(term.clone(), name, SourceKind::Senado);
        if !seen.insert(senator.normalized_name.clone()) {
            duplicates += 1;
            continue;
        }
        senator.party = card_party(card);
        let (phone, extension) = card_phone(card);
        senator.phone = phone;
        senator.phone_extension = extension;
        senator.website = card_website(card);
        senators.push(senator);
    }

    info!(
        cards = total,
        records = senators.len(),
        skipped,
        duplicates,
        "senado cards parsed"
    );
    if senators.is_empty() {
        return Err(SourceError::Empty);
    }
    Ok(senators)
}

fn selector(css: &str) -> Selector {
    Selector::parse(css).expect("static selector is valid")
}

fn element_text(element: ElementRef<'_>) -> String {
    element.text().collect::<String>()
}

fn card_name(card: ElementRef<'_>) -> Option<String> {
    let h3 = card.select(&selector("h3")).next()?;
    clean_text(&element_text(h3))
}

fn card_party(card: ElementRef<'_>) -> Option<String> {
    let h3 = card.select(&selector("h3")).next()?;
    let party_node = h3
        .next_siblings()
        .filter_map(ElementRef::wrap)
        .find(|e| e.value().name() == "p")?;
    clean_text(&element_text(party_node))
}

fn card_phone(card: ElementRef<'_>) -> (Option<String>, Option<String>) {
    let label_sel = selector(PHONE_LABEL);
    let Some(label) = card.select(&label_sel).next() else {
        return (None, None);
    };
    let label_text = element_text(label);
    let Some(paragraph) = label.parent().and_then(ElementRef::wrap) else {
        return (None, None);
    };
    let text = element_text(paragraph).replacen(&label_text, "", 1);
    split_extension(&text)
}

fn split_extension(text: &str) -> (Option<String>, Option<String>) {
    let upper = text.to_uppercase();
    match upper.find("EXT") {
        Some(index) => {
            let phone = clean_text(&text[..index]);
            let extension = clean_text(&text[index + 3..]);
            (phone, extension)
        }
        None => (clean_text(text), None),
    }
}

fn card_website(card: ElementRef<'_>) -> Option<String> {
    card.select(&selector("p")).find_map(|p| {
        let text = clean_text(&element_text(p))?;
        let lower = text.to_lowercase();
        if lower.starts_with("http") || lower.starts_with("www") {
            Some(text)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../../../tests/fixtures/senado_senators.html");

    fn term() -> Term {
        Term::parse("2026-2030").unwrap()
    }

    fn card(inner: &str) -> String {
        format!(
            r#"<html><body><div class="modazdirectory__results"><div class="modazdirectory__result">
                <div>{inner}</div></div></div></body></html>"#
        )
    }

    #[test]
    fn fixture_yields_unique_senators_and_dedupes_the_repeated_card() {
        let senators = parse_html(FIXTURE, &term()).unwrap();
        assert_eq!(senators.len(), 6);
        let names: Vec<&str> = senators.iter().map(|s| s.full_name.as_str()).collect();
        assert_eq!(
            names
                .iter()
                .filter(|n| **n == "Agudelo García Ana Paola")
                .count(),
            1
        );
    }

    #[test]
    fn fixture_card_fields_are_extracted() {
        let senators = parse_html(FIXTURE, &term()).unwrap();
        let agudelo = senators
            .iter()
            .find(|s| s.full_name == "Agudelo García Ana Paola")
            .unwrap();
        assert_eq!(agudelo.party.as_deref(), Some("MIRA"));
        assert_eq!(agudelo.phone.as_deref(), Some("(57)(601) 3823000"));
        assert_eq!(agudelo.phone_extension.as_deref(), Some("3610"));
        assert_eq!(agudelo.email, None, "emails are not decoded (decision D3)");
        assert_eq!(agudelo.id.as_str(), "2026-2030-agudelo-garcia-ana-paola");
        assert_eq!(agudelo.source, SourceKind::Senado);
    }

    #[test]
    fn fixture_empty_party_and_website_cases() {
        let senators = parse_html(FIXTURE, &term()).unwrap();
        assert!(
            senators.iter().any(|s| s.party.is_none()),
            "one empty party is None"
        );
        assert!(
            senators
                .iter()
                .any(|s| s.website.as_deref() == Some("https://www.ejemplo.gov.co")),
            "a card with a website keeps it"
        );
    }

    #[test]
    fn no_container_is_parse_error() {
        let result = parse_html("<html><body><p>nothing</p></body></html>", &term());
        assert!(matches!(result, Err(SourceError::Parse(_))));
    }

    #[test]
    fn container_without_cards_is_empty() {
        let result = parse_html(r#"<div class="modazdirectory__results"></div>"#, &term());
        assert_eq!(result, Err(SourceError::Empty));
    }

    #[test]
    fn card_without_name_is_skipped() {
        let html = card("<h3>   </h3><p>Partido</p>");
        assert_eq!(parse_html(&html, &term()), Err(SourceError::Empty));
    }

    #[test]
    fn card_without_name_is_skipped_but_others_kept() {
        let html = format!(
            "{}{}",
            card("<h3>   </h3><p>Partido</p>"),
            card("<h3>Ana Paola</h3><p>MIRA</p>")
        );
        let senators = parse_html(&html, &term()).unwrap();
        assert_eq!(senators.len(), 1);
        assert_eq!(senators[0].party.as_deref(), Some("MIRA"));
    }

    #[test]
    fn extension_range_is_kept_as_published() {
        let html = card(
            r#"<h3>Ana Paola</h3><p>MIRA</p>
               <p><span class="modazdirectory__label-phone">t: </span>(57)(601) 3823000 EXT 5051 - 5052</p>"#,
        );
        let senators = parse_html(&html, &term()).unwrap();
        assert_eq!(senators[0].phone.as_deref(), Some("(57)(601) 3823000"));
        assert_eq!(senators[0].phone_extension.as_deref(), Some("5051 - 5052"));
    }

    #[test]
    fn phone_without_extension() {
        let html = card(
            r#"<h3>Ana Paola</h3><p>MIRA</p>
               <p><span class="modazdirectory__label-phone">t: </span>(57)(601) 3823000</p>"#,
        );
        let senators = parse_html(&html, &term()).unwrap();
        assert_eq!(senators[0].phone.as_deref(), Some("(57)(601) 3823000"));
        assert_eq!(senators[0].phone_extension, None);
    }

    #[test]
    fn extension_match_is_case_insensitive() {
        assert_eq!(
            split_extension("555 ext 9"),
            (Some("555".into()), Some("9".into()))
        );
        assert_eq!(split_extension("555"), (Some("555".into()), None));
    }

    #[test]
    fn website_is_taken_from_http_or_www_paragraph() {
        let html = card(r#"<h3>Ana Paola</h3><p>MIRA</p><p>www.ana.co</p>"#);
        let senators = parse_html(&html, &term()).unwrap();
        assert_eq!(senators[0].website.as_deref(), Some("www.ana.co"));
        let none = card(r#"<h3>Luis</h3><p>MIRA</p><p>Edificio</p>"#);
        assert_eq!(parse_html(&none, &term()).unwrap()[0].website, None);
    }

    #[test]
    fn name_with_extra_spaces_is_trimmed() {
        let html = card("<h3>  Ana   Paola  </h3>");
        assert_eq!(
            parse_html(&html, &term()).unwrap()[0].full_name,
            "Ana Paola"
        );
    }

    #[test]
    fn duplicate_names_are_counted_and_first_kept() {
        let html = format!(
            "{}{}",
            card("<h3>Ana Paola</h3><p>MIRA</p>"),
            card("<h3>ANA  PAOLA</h3><p>Otro</p>")
        );
        let senators = parse_html(&html, &term()).unwrap();
        assert_eq!(senators.len(), 1);
        assert_eq!(senators[0].party.as_deref(), Some("MIRA"));
    }
}
