use std::collections::HashSet;

use serde::Deserialize;
use tracing::{info, warn};

use crate::application::errors::SourceError;
use crate::domain::normalize::clean_text;
use crate::domain::representative::Representative;
use crate::domain::senator::{SourceKind, Term};

#[derive(Debug, Deserialize)]
pub struct RawRepresentativeRow {
    pub sesion: Option<String>,
    pub partido_politico: Option<String>,
    pub departamento: Option<String>,
}

pub fn parse_representatives_body(
    body: &str,
    term: &Term,
) -> Result<Vec<Representative>, SourceError> {
    let rows: Vec<RawRepresentativeRow> = serde_json::from_str(body)
        .map_err(|_| SourceError::Parse("body is not a JSON array of rows".into()))?;
    Ok(map_representative_rows(rows, term))
}

pub fn map_representative_rows(
    rows: Vec<RawRepresentativeRow>,
    term: &Term,
) -> Vec<Representative> {
    let mut seen = HashSet::new();
    let mut representatives = Vec::new();
    let mut skipped = 0;
    let mut duplicates = 0;

    for (position, row) in rows.into_iter().enumerate() {
        let Some(full_name) = row.sesion.as_deref().and_then(clean_text) else {
            skipped += 1;
            warn!(position, "row without a usable name skipped");
            continue;
        };
        let mut representative =
            Representative::new(term.clone(), full_name, SourceKind::DatosGovCo);
        if !seen.insert(representative.normalized_name.clone()) {
            duplicates += 1;
            continue;
        }
        representative.party = row.partido_politico.as_deref().and_then(clean_text);
        representative.constituency = row.departamento.as_deref().and_then(clean_text);
        representatives.push(representative);
    }

    info!(
        records = representatives.len(),
        skipped, duplicates, "datos.gov.co representative rows mapped"
    );
    representatives
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str =
        include_str!("../../../../tests/fixtures/datos_gov_co_representatives.json");

    fn term() -> Term {
        Term::parse("2026-2030").unwrap()
    }

    fn row(json: &str) -> RawRepresentativeRow {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn fixture_maps_all_rows() {
        let reps = parse_representatives_body(FIXTURE, &term()).unwrap();
        assert_eq!(reps.len(), 181);
        let rep = reps
            .iter()
            .find(|r| r.full_name == "ABASOLO GÓMEZ ALEJANDRA GABRIELA")
            .unwrap();
        assert_eq!(rep.party.as_deref(), Some("ALIANZA VERDE"));
        assert_eq!(rep.constituency.as_deref(), Some("NARIÑO"));
        assert_eq!(
            rep.id.as_str(),
            "2026-2030-abasolo-gomez-alejandra-gabriela"
        );
        assert_eq!(rep.source, SourceKind::DatosGovCo);
        assert_eq!(rep.gender, crate::domain::senator::Gender::Unknown);
        assert_eq!(rep.email, None);
    }

    #[test]
    fn session_columns_are_ignored() {
        let mapped = map_representative_rows(
            vec![row(
                r#"{"sesion":"Ana Paola","partido_politico":"MIRA","departamento":"NARIÑO","sesi_n_plenaria_19_08_2026":"ASISTIO"}"#,
            )],
            &term(),
        );
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].party.as_deref(), Some("MIRA"));
    }

    #[test]
    fn nd_and_blank_values_become_none() {
        let mapped = map_representative_rows(
            vec![row(
                r#"{"sesion":"Luis Pérez","partido_politico":"ND","departamento":"  "}"#,
            )],
            &term(),
        );
        assert_eq!(mapped[0].party, None);
        assert_eq!(mapped[0].constituency, None);
    }

    #[test]
    fn citrep_row_maps_its_list_name() {
        let mapped = map_representative_rows(
            vec![row(
                r#"{"sesion":"Ana Lucía","partido_politico":"CITREP 9- FUNDACIÓN RENACER SOCIAL DEL PACIFICO COLOMBIANO","departamento":"VALLE Y CAUCA"}"#,
            )],
            &term(),
        );
        assert!(mapped[0].party.as_deref().unwrap().starts_with("CITREP 9"));
        assert_eq!(mapped[0].constituency.as_deref(), Some("VALLE Y CAUCA"));
    }

    #[test]
    fn blank_names_are_skipped_and_duplicates_keep_first() {
        let mapped = map_representative_rows(
            vec![
                row(r#"{"sesion":"  "}"#),
                row(r#"{"partido_politico":"X"}"#),
                row(r#"{"sesion":"Luis Pérez","partido_politico":"A"}"#),
                row(r#"{"sesion":"LUIS  PÉREZ","partido_politico":"B"}"#),
            ],
            &term(),
        );
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].party.as_deref(), Some("A"));
    }

    #[test]
    fn invalid_shape_is_parse_error() {
        assert!(matches!(
            parse_representatives_body("{}", &term()),
            Err(SourceError::Parse(_))
        ));
        assert!(
            parse_representatives_body("[]", &term())
                .unwrap()
                .is_empty()
        );
    }
}
