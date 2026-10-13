use chrono::{DateTime, NaiveDate, Utc};

use super::normalize::normalize_name;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gender {
    Male,
    Female,
    Unknown,
}

impl Gender {
    pub fn as_str(&self) -> &'static str {
        match self {
            Gender::Male => "male",
            Gender::Female => "female",
            Gender::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    DatosGovCo,
    Senado,
    Registraduria,
    CongresoVisible,
}

impl SourceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SourceKind::DatosGovCo => "datos_gov_co",
            SourceKind::Senado => "senado",
            SourceKind::Registraduria => "registraduria",
            SourceKind::CongresoVisible => "congreso_visible",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Term(String);

impl Term {
    pub fn parse(input: &str) -> Option<Term> {
        let (start, end) = input.split_once('-')?;
        let valid = |y: &str| y.len() == 4 && y.chars().all(|c| c.is_ascii_digit());
        if valid(start) && valid(end) {
            Some(Term(input.to_string()))
        } else {
            None
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SenatorId(String);

impl SenatorId {
    pub fn derive(term: &Term, normalized_name: &str) -> SenatorId {
        let slug = slugify(normalized_name);
        SenatorId(format!("{}-{}", term.as_str(), slug))
    }

    pub fn from_stored(value: String) -> SenatorId {
        SenatorId(value)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn slugify(input: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = true;
    for c in input.chars() {
        if c.is_alphanumeric() {
            slug.push(c);
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    slug.trim_end_matches('-').to_string()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Senator {
    pub id: SenatorId,
    pub term: Term,
    pub full_name: String,
    pub normalized_name: String,
    pub gender: Gender,
    pub birth_date: Option<NaiveDate>,
    pub department_of_birth: Option<String>,
    pub city_of_birth: Option<String>,
    pub party: Option<String>,
    pub votes: Option<u64>,
    pub constitutional_commission: Option<String>,
    pub legal_commission: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub phone_extension: Option<String>,
    pub website: Option<String>,
    pub twitter: Option<String>,
    pub facebook: Option<String>,
    pub source: SourceKind,
    pub updated_at: Option<DateTime<Utc>>,
}

impl Senator {
    pub fn new(term: Term, full_name: String, source: SourceKind) -> Senator {
        let normalized_name = normalize_name(&full_name);
        let id = SenatorId::derive(&term, &normalized_name);
        Senator {
            id,
            term,
            full_name,
            normalized_name,
            gender: Gender::Unknown,
            birth_date: None,
            department_of_birth: None,
            city_of_birth: None,
            party: None,
            votes: None,
            constitutional_commission: None,
            legal_commission: None,
            email: None,
            phone: None,
            phone_extension: None,
            website: None,
            twitter: None,
            facebook: None,
            source,
            updated_at: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn term_accepts_valid_and_rejects_invalid() {
        assert_eq!(Term::parse("2026-2030").unwrap().as_str(), "2026-2030");
        for bad in ["2026", "2026-30", "abc", "2026-20a0", "", "2026_2030"] {
            assert!(Term::parse(bad).is_none(), "should reject {bad:?}");
        }
    }

    #[test]
    fn senator_id_slug_for_accents_and_punctuation() {
        let term = Term::parse("2026-2030").unwrap();
        let s = Senator::new(
            term,
            "Acuña  Díaz, Laureano (Augusto)".into(),
            SourceKind::Senado,
        );
        assert_eq!(s.normalized_name, "acuna diaz, laureano (augusto)");
        assert_eq!(s.id.as_str(), "2026-2030-acuna-diaz-laureano-augusto");
    }

    #[test]
    fn senator_new_sets_defaults() {
        let term = Term::parse("2018-2022").unwrap();
        let s = Senator::new(term, "Ana Paola".into(), SourceKind::DatosGovCo);
        assert_eq!(s.gender, Gender::Unknown);
        assert!(s.party.is_none());
        assert_eq!(s.source.as_str(), "datos_gov_co");
        assert_eq!(Gender::Female.as_str(), "female");
        assert_eq!(SourceKind::Senado.as_str(), "senado");
        assert_eq!(SourceKind::Registraduria.as_str(), "registraduria");
        assert_eq!(SourceKind::CongresoVisible.as_str(), "congreso_visible");
    }
}
