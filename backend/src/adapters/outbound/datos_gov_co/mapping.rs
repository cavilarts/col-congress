use std::collections::HashSet;

use serde::Deserialize;
use tracing::{info, warn};

use crate::application::errors::SourceError;
use crate::domain::normalize::{clean_text, parse_birth_date_es, parse_gender, parse_votes};
use crate::domain::senator::{Gender, Senator, SourceKind, Term};

#[derive(Debug, Deserialize)]
pub struct RawRow {
    pub nombre: Option<String>,
    pub g_nero: Option<String>,
    pub a_o_de_nacimiento: Option<String>,
    pub departamento_de_nacimiento: Option<String>,
    pub ciudad_de_nacimiento: Option<String>,
    pub partido: Option<String>,
    pub n_mero_de_votos: Option<String>,
    pub comisi_n_constitucional: Option<String>,
    pub comisi_n_legal: Option<String>,
    pub correo_electr_nico: Option<String>,
    pub twitter: Option<String>,
    pub facebook: Option<String>,
    pub pagina_web: Option<String>,
    pub telefono: Option<String>,
    pub ext: Option<String>,
}

pub fn parse_body(body: &str, term: &Term) -> Result<Vec<Senator>, SourceError> {
    let rows: Vec<RawRow> = serde_json::from_str(body)
        .map_err(|_| SourceError::Parse("body is not a JSON array of rows".into()))?;
    Ok(map_rows(rows, term))
}

pub fn map_rows(rows: Vec<RawRow>, term: &Term) -> Vec<Senator> {
    let mut seen = HashSet::new();
    let mut senators = Vec::new();
    let mut skipped = 0;
    let mut duplicates = 0;

    for (position, row) in rows.into_iter().enumerate() {
        let Some(full_name) = row.nombre.as_deref().and_then(clean_text) else {
            skipped += 1;
            warn!(position, "row without a usable name skipped");
            continue;
        };
        let mut senator = Senator::new(term.clone(), full_name, SourceKind::DatosGovCo);
        if !seen.insert(senator.normalized_name.clone()) {
            duplicates += 1;
            continue;
        }

        senator.gender = row.g_nero.as_deref().map_or(Gender::Unknown, parse_gender);
        senator.birth_date = row
            .a_o_de_nacimiento
            .as_deref()
            .and_then(clean_text)
            .and_then(|text| {
                let parsed = parse_birth_date_es(&text);
                if parsed.is_none() {
                    warn!(
                        id = senator.id.as_str(),
                        "unparseable birth date stored as null"
                    );
                }
                parsed
            });
        senator.department_of_birth = row
            .departamento_de_nacimiento
            .as_deref()
            .and_then(clean_text);
        senator.city_of_birth = row.ciudad_de_nacimiento.as_deref().and_then(clean_text);
        senator.party = row.partido.as_deref().and_then(clean_text);
        senator.votes = row
            .n_mero_de_votos
            .as_deref()
            .and_then(clean_text)
            .and_then(|text| parse_votes(&text));
        senator.constitutional_commission =
            row.comisi_n_constitucional.as_deref().and_then(clean_text);
        senator.legal_commission = row.comisi_n_legal.as_deref().and_then(clean_text);
        senator.email = row.correo_electr_nico.as_deref().and_then(clean_text);
        senator.phone = row.telefono.as_deref().and_then(clean_text);
        senator.phone_extension = row.ext.as_deref().and_then(clean_text);
        senator.website = row.pagina_web.as_deref().and_then(clean_text);
        senator.twitter = row.twitter.as_deref().and_then(clean_text);
        senator.facebook = row.facebook.as_deref().and_then(clean_text);
        senators.push(senator);
    }

    info!(
        records = senators.len(),
        skipped, duplicates, "datos.gov.co rows mapped"
    );
    senators
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    const FIXTURE: &str = include_str!("../../../../tests/fixtures/datos_gov_co_senators.json");

    fn term() -> Term {
        Term::parse("2018-2022").unwrap()
    }

    fn row(json: &str) -> RawRow {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn fixture_maps_every_row_with_expected_fields() {
        let senators = parse_body(FIXTURE, &term()).unwrap();
        assert_eq!(senators.len(), 105);

        let acuna = senators
            .iter()
            .find(|s| s.full_name == "Acuña Díaz Laureano Augusto")
            .unwrap();
        assert_eq!(acuna.gender, Gender::Male);
        assert_eq!(acuna.birth_date, NaiveDate::from_ymd_opt(1971, 3, 25));
        assert_eq!(acuna.votes, Some(82854));
        assert_eq!(acuna.party.as_deref(), Some("Conservador"));
        assert_eq!(acuna.website, None, "pagina_web ND becomes null");
        assert_eq!(acuna.phone_extension.as_deref(), Some("0"));
        assert_eq!(acuna.id.as_str(), "2018-2022-acuna-diaz-laureano-augusto");
        assert_eq!(acuna.source, SourceKind::DatosGovCo);
    }

    #[test]
    fn fixture_contains_placeholder_and_invalid_date_rows() {
        let senators = parse_body(FIXTURE, &term()).unwrap();
        let nd_date = senators.iter().filter(|s| s.birth_date.is_none()).count();
        assert_eq!(
            nd_date, 5,
            "'ND', two 'Novienbre' typos, '22 de noviembre de 1066' (year out of range) and '18 de Julio e 1950' become null"
        );
        let lower_gender = senators.iter().filter(|s| s.gender == Gender::Male).count();
        assert!(lower_gender > 0);
    }

    #[test]
    fn nd_values_become_none() {
        let mapped = map_rows(
            vec![row(
                r#"{"nombre":"Ana Paola ","g_nero":"ND","partido":"N/D","pagina_web":"ND","n_mero_de_votos":"ND"}"#,
            )],
            &term(),
        );
        let s = &mapped[0];
        assert_eq!(s.full_name, "Ana Paola");
        assert_eq!(s.gender, Gender::Unknown);
        assert_eq!(s.party, None);
        assert_eq!(s.website, None);
        assert_eq!(s.votes, None);
    }

    #[test]
    fn invalid_birth_date_is_null_and_record_kept() {
        let mapped = map_rows(
            vec![row(
                r#"{"nombre":"Luis Pérez","a_o_de_nacimiento":"22 de noviembre de 1066"}"#,
            )],
            &term(),
        );
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].birth_date, None);
    }

    #[test]
    fn blank_and_missing_names_are_skipped() {
        let mapped = map_rows(
            vec![
                row(r#"{"nombre":"   "}"#),
                row(r#"{"g_nero":"Masculino"}"#),
                row(r#"{"nombre":"Luis Pérez"}"#),
            ],
            &term(),
        );
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].full_name, "Luis Pérez");
    }

    #[test]
    fn duplicate_names_keep_first_occurrence() {
        let mapped = map_rows(
            vec![
                row(r#"{"nombre":"Luis Pérez","partido":"A"}"#),
                row(r#"{"nombre":"LUIS  PEREZ","partido":"B"}"#),
            ],
            &term(),
        );
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].party.as_deref(), Some("A"));
    }

    #[test]
    fn invalid_json_shape_is_parse_error() {
        assert!(matches!(
            parse_body("{\"not\":\"an array\"}", &term()),
            Err(SourceError::Parse(_))
        ));
        assert!(matches!(
            parse_body("not json", &term()),
            Err(SourceError::Parse(_))
        ));
    }

    #[test]
    fn empty_array_maps_to_no_senators() {
        assert!(parse_body("[]", &term()).unwrap().is_empty());
    }
}
