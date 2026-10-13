use chrono::{DateTime, NaiveDate, Utc};

use super::normalize::normalize_name;
use super::senator::{Gender, SourceKind, Term};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepresentativeId(String);

impl RepresentativeId {
    pub fn derive(term: &Term, normalized_name: &str) -> RepresentativeId {
        RepresentativeId(format!("{}-{}", term.as_str(), slugify(normalized_name)))
    }

    pub fn from_stored(value: String) -> RepresentativeId {
        RepresentativeId(value)
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
pub struct Representative {
    pub id: RepresentativeId,
    pub term: Term,
    pub full_name: String,
    pub normalized_name: String,
    pub gender: Gender,
    pub birth_date: Option<NaiveDate>,
    pub department_of_birth: Option<String>,
    pub city_of_birth: Option<String>,
    pub party: Option<String>,
    pub constituency: Option<String>,
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

impl Representative {
    pub fn new(term: Term, full_name: String, source: SourceKind) -> Representative {
        let normalized_name = normalize_name(&full_name);
        let id = RepresentativeId::derive(&term, &normalized_name);
        Representative {
            id,
            term,
            full_name,
            normalized_name,
            gender: Gender::Unknown,
            birth_date: None,
            department_of_birth: None,
            city_of_birth: None,
            party: None,
            constituency: None,
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
    fn id_slug_for_upper_case_accented_names() {
        let term = Term::parse("2026-2030").unwrap();
        let r = Representative::new(
            term,
            "ABASOLO GÓMEZ ALEJANDRA GABRIELA".into(),
            SourceKind::DatosGovCo,
        );
        assert_eq!(r.normalized_name, "abasolo gomez alejandra gabriela");
        assert_eq!(r.id.as_str(), "2026-2030-abasolo-gomez-alejandra-gabriela");
        assert_eq!(r.constituency, None);
        assert_eq!(r.updated_at, None);
    }

    #[test]
    fn from_stored_keeps_value() {
        assert_eq!(RepresentativeId::from_stored("x".into()).as_str(), "x");
    }

    #[test]
    fn slug_handles_punctuation() {
        let term = Term::parse("2026-2030").unwrap();
        let id = RepresentativeId::derive(&term, "jose luis 'joche' abdala-olivera");
        assert_eq!(id.as_str(), "2026-2030-jose-luis-joche-abdala-olivera");
    }
}
