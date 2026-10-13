use chrono::{Datelike, NaiveDate, Utc};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use super::senator::Gender;

pub fn clean_text(input: &str) -> Option<String> {
    let collapsed = input.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    if collapsed.eq_ignore_ascii_case("ND") || collapsed.eq_ignore_ascii_case("N/D") {
        return None;
    }
    Some(collapsed)
}

pub fn parse_gender(input: &str) -> Gender {
    match input.trim().to_lowercase().as_str() {
        "masculino" | "male" => Gender::Male,
        "femenino" | "female" => Gender::Female,
        _ => Gender::Unknown,
    }
}

pub fn parse_votes(input: &str) -> Option<u64> {
    let digits: String = input
        .trim()
        .chars()
        .filter(|c| *c != ',' && *c != '.')
        .collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

pub fn parse_birth_date_es(input: &str) -> Option<NaiveDate> {
    parse_birth_date_es_until(input, Utc::now().year() - 18)
}

fn parse_birth_date_es_until(input: &str, max_year: i32) -> Option<NaiveDate> {
    let tokens: Vec<String> = input.split_whitespace().map(|t| t.to_lowercase()).collect();
    let mut rest = tokens.as_slice();
    let day: u32 = match rest.first()?.parse() {
        Ok(d) => d,
        Err(_) => return None,
    };
    rest = &rest[1..];
    rest = strip_de(rest);
    let month = month_number(rest.first()?)?;
    rest = &rest[1..];
    rest = strip_de(rest);
    if rest.len() != 1 {
        return None;
    }
    let year: i32 = rest[0].parse().ok()?;
    if rest[0].len() != 4 || year < 1900 || year > max_year {
        return None;
    }
    NaiveDate::from_ymd_opt(year, month, day)
}

fn strip_de(tokens: &[String]) -> &[String] {
    match tokens.first() {
        Some(t) if t == "de" || t == "del" => &tokens[1..],
        _ => tokens,
    }
}

fn month_number(name: &str) -> Option<u32> {
    let normalized = normalize_name(name);
    Some(match normalized.as_str() {
        "enero" => 1,
        "febrero" => 2,
        "marzo" => 3,
        "abril" => 4,
        "mayo" => 5,
        "junio" => 6,
        "julio" => 7,
        "agosto" => 8,
        "septiembre" | "setiembre" => 9,
        "octubre" => 10,
        "noviembre" => 11,
        "diciembre" => 12,
        _ => return None,
    })
}

pub fn normalize_name(input: &str) -> String {
    input
        .nfd()
        .filter(|c| !is_combining_mark(*c))
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_text_cases() {
        let cases: &[(&str, Option<&str>)] = &[
            ("  Polo  Democrático ", Some("Polo Democrático")),
            ("ND", None),
            ("n/d", None),
            ("", None),
            ("   ", None),
            ("MIRA", Some("MIRA")),
        ];
        for (input, expected) in cases {
            assert_eq!(clean_text(input).as_deref(), *expected, "input {input:?}");
        }
    }

    #[test]
    fn parse_gender_cases() {
        let cases: &[(&str, Gender)] = &[
            ("masculino", Gender::Male),
            ("Femenino", Gender::Female),
            ("  MALE ", Gender::Male),
            ("female", Gender::Female),
            ("ND", Gender::Unknown),
            ("otro", Gender::Unknown),
        ];
        for (input, expected) in cases {
            assert_eq!(parse_gender(input), *expected, "input {input:?}");
        }
    }

    #[test]
    fn parse_votes_cases() {
        let cases: &[(&str, Option<u64>)] = &[
            ("82,854", Some(82854)),
            ("82.854", Some(82854)),
            (" 12 ", Some(12)),
            ("ND", None),
            ("abc", None),
            ("", None),
        ];
        for (input, expected) in cases {
            assert_eq!(parse_votes(input), *expected, "input {input:?}");
        }
    }

    #[test]
    fn parse_birth_date_cases() {
        let cases: &[(&str, Option<NaiveDate>)] = &[
            ("25 de marzo de 1971", NaiveDate::from_ymd_opt(1971, 3, 25)),
            ("8 DE MAYO DE 1983", NaiveDate::from_ymd_opt(1983, 5, 8)),
            ("3 Enero 1960", NaiveDate::from_ymd_opt(1960, 1, 3)),
            (
                "1 de setiembre de 1950",
                NaiveDate::from_ymd_opt(1950, 9, 1),
            ),
            ("22 de noviembre de 1066", None),
            ("18 de Julio e 1950", None),
            ("31 de febrero de 1970", None),
            ("ND", None),
            ("25 de marzo de 2020", None),
            ("25 de marzo de 19", None),
            ("25 de marzo", None),
            ("25 de Marzo de 1971 extra", None),
            ("x de marzo de 1971", None),
            ("25 de Marzoo de 1971", None),
            (
                "4 de noviembre del 1968",
                NaiveDate::from_ymd_opt(1968, 11, 4),
            ),
            ("", None),
        ];
        for (input, expected) in cases {
            assert_eq!(
                parse_birth_date_es_until(input, 2008),
                *expected,
                "input {input:?}"
            );
        }
    }

    #[test]
    fn parse_birth_date_public_uses_current_year_minus_18() {
        assert!(parse_birth_date_es("25 de marzo de 1971").is_some());
        assert!(parse_birth_date_es("25 de marzo de 2100").is_none());
    }

    #[test]
    fn normalize_name_cases() {
        let cases: &[(&str, &str)] = &[
            ("Acuña Díaz Laureano Augusto", "acuna diaz laureano augusto"),
            ("  Agudelo   García Ana Paola ", "agudelo garcia ana paola"),
            ("ÁVILA MARTÍNEZ", "avila martinez"),
        ];
        for (input, expected) in cases {
            assert_eq!(normalize_name(input), *expected, "input {input:?}");
        }
    }
}
