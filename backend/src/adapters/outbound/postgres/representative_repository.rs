use chrono::{DateTime, NaiveDate, Utc};
use sqlx::pool::PoolConnection;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Postgres, Row, Transaction};

use super::senator_repository::{SyncLock, storage};
use crate::application::errors::RepositoryError;
use crate::application::ports::RepresentativeRepository;
use crate::domain::representative::{Representative, RepresentativeId};
use crate::domain::senator::{Gender, SourceKind, Term};

pub const REPRESENTATIVES_SYNC_LOCK_KEY: i64 = 0x5E4A_7A02;

#[derive(Clone)]
pub struct PgRepresentativeRepository {
    pool: PgPool,
}

impl PgRepresentativeRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const INSERT: &str = "INSERT INTO representatives (
    id, term, normalized_name, full_name, gender, birth_date, department_of_birth,
    city_of_birth, party, constituency, votes, constitutional_commission, legal_commission,
    email, phone, phone_extension, website, twitter, facebook, source, updated_at
) VALUES (
    $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21
) ON CONFLICT (id) DO UPDATE SET
    term = EXCLUDED.term,
    normalized_name = EXCLUDED.normalized_name,
    full_name = EXCLUDED.full_name,
    gender = EXCLUDED.gender,
    birth_date = EXCLUDED.birth_date,
    department_of_birth = EXCLUDED.department_of_birth,
    city_of_birth = EXCLUDED.city_of_birth,
    party = EXCLUDED.party,
    constituency = EXCLUDED.constituency,
    votes = EXCLUDED.votes,
    constitutional_commission = EXCLUDED.constitutional_commission,
    legal_commission = EXCLUDED.legal_commission,
    email = EXCLUDED.email,
    phone = EXCLUDED.phone,
    phone_extension = EXCLUDED.phone_extension,
    website = EXCLUDED.website,
    twitter = EXCLUDED.twitter,
    facebook = EXCLUDED.facebook,
    source = EXCLUDED.source,
    updated_at = EXCLUDED.updated_at";

async fn insert(
    tx: &mut Transaction<'_, Postgres>,
    representative: &Representative,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(INSERT)
        .bind(representative.id.as_str())
        .bind(representative.term.as_str())
        .bind(&representative.normalized_name)
        .bind(&representative.full_name)
        .bind(representative.gender.as_str())
        .bind(representative.birth_date)
        .bind(&representative.department_of_birth)
        .bind(&representative.city_of_birth)
        .bind(&representative.party)
        .bind(&representative.constituency)
        .bind(representative.votes.map(|v| v as i64))
        .bind(&representative.constitutional_commission)
        .bind(&representative.legal_commission)
        .bind(&representative.email)
        .bind(&representative.phone)
        .bind(&representative.phone_extension)
        .bind(&representative.website)
        .bind(&representative.twitter)
        .bind(&representative.facebook)
        .bind(representative.source.as_str())
        .bind(now)
        .execute(&mut **tx)
        .await
        .map(|_| ())
}

fn gender_from(value: &str) -> Gender {
    match value {
        "male" => Gender::Male,
        "female" => Gender::Female,
        _ => Gender::Unknown,
    }
}

fn source_from(value: &str) -> SourceKind {
    match value {
        "datos_gov_co" => SourceKind::DatosGovCo,
        _ => SourceKind::CongresoVisible,
    }
}

fn map_row(row: &PgRow) -> Result<Representative, sqlx::Error> {
    let term_text: String = row.try_get("term")?;
    let term =
        Term::parse(&term_text).ok_or_else(|| sqlx::Error::Decode("invalid term in row".into()))?;
    let id: String = row.try_get("id")?;
    let gender: String = row.try_get("gender")?;
    let source: String = row.try_get("source")?;
    let birth_date: Option<NaiveDate> = row.try_get("birth_date")?;
    let votes: Option<i64> = row.try_get("votes")?;
    Ok(Representative {
        id: RepresentativeId::from_stored(id),
        term,
        full_name: row.try_get("full_name")?,
        normalized_name: row.try_get("normalized_name")?,
        gender: gender_from(&gender),
        birth_date,
        department_of_birth: row.try_get("department_of_birth")?,
        city_of_birth: row.try_get("city_of_birth")?,
        party: row.try_get("party")?,
        constituency: row.try_get("constituency")?,
        votes: votes.map(|v| v as u64),
        constitutional_commission: row.try_get("constitutional_commission")?,
        legal_commission: row.try_get("legal_commission")?,
        email: row.try_get("email")?,
        phone: row.try_get("phone")?,
        phone_extension: row.try_get("phone_extension")?,
        website: row.try_get("website")?,
        twitter: row.try_get("twitter")?,
        facebook: row.try_get("facebook")?,
        source: source_from(&source),
        updated_at: Some(row.try_get("updated_at")?),
    })
}

impl RepresentativeRepository for PgRepresentativeRepository {
    type Lock = SyncLock;

    async fn last_updated(&self, term: &Term) -> Result<Option<DateTime<Utc>>, RepositoryError> {
        sqlx::query_scalar("SELECT MAX(updated_at) FROM representatives WHERE term = $1")
            .bind(term.as_str())
            .fetch_one(&self.pool)
            .await
            .map_err(storage)
    }

    async fn upsert_all(
        &self,
        representatives: &[Representative],
        now: DateTime<Utc>,
    ) -> Result<usize, RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(storage)?;
        for representative in representatives {
            insert(&mut tx, representative, now)
                .await
                .map_err(storage)?;
        }
        tx.commit().await.map_err(storage)?;
        Ok(representatives.len())
    }

    async fn list(&self, term: Option<&Term>) -> Result<Vec<Representative>, RepositoryError> {
        const SELECT: &str = "SELECT * FROM representatives";
        const ORDER: &str = " ORDER BY term DESC, full_name";
        let rows = match term {
            Some(term) => {
                sqlx::query(&format!("{SELECT} WHERE term = $1{ORDER}"))
                    .bind(term.as_str())
                    .fetch_all(&self.pool)
                    .await
            }
            None => {
                sqlx::query(&format!("{SELECT}{ORDER}"))
                    .fetch_all(&self.pool)
                    .await
            }
        }
        .map_err(storage)?;
        rows.iter()
            .map(map_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage)
    }

    async fn try_sync_lock(&self) -> Result<Option<SyncLock>, RepositoryError> {
        let mut connection: PoolConnection<Postgres> =
            self.pool.acquire().await.map_err(storage)?;
        let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
            .bind(REPRESENTATIVES_SYNC_LOCK_KEY)
            .fetch_one(&mut *connection)
            .await
            .map_err(storage)?;
        if !acquired {
            return Ok(None);
        }
        Ok(Some(SyncLock::new(connection.detach())))
    }
}

#[cfg(test)]
mod integration {
    use chrono::TimeZone;

    use super::*;
    use crate::adapters::outbound::postgres::test_support::test_pool as pool;

    macro_rules! require_db {
        () => {
            match pool().await {
                Some(value) => value,
                None => {
                    eprintln!("skipping: TEST_DATABASE_URL is not set");
                    return;
                }
            }
        };
    }

    fn rep(term: &str, name: &str) -> Representative {
        let mut r = Representative::new(
            Term::parse(term).unwrap(),
            name.into(),
            SourceKind::DatosGovCo,
        );
        r.constituency = Some("NARIÑO".into());
        r.birth_date = NaiveDate::from_ymd_opt(1970, 1, 2);
        r.votes = Some(5);
        r
    }

    fn at(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 11, hour, 0, 0).unwrap()
    }

    #[tokio::test]
    async fn upsert_twice_creates_no_duplicates() {
        let (pool, _guard) = require_db!();
        let repo = PgRepresentativeRepository::new(pool.clone());
        let batch = vec![
            rep("2026-2030", "Ana Paola"),
            rep("2026-2030", "Luis Pérez"),
        ];
        repo.upsert_all(&batch, at(7)).await.unwrap();
        assert_eq!(repo.upsert_all(&batch, at(8)).await.unwrap(), 2);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM representatives")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn upsert_updates_fields_and_last_updated() {
        let (pool, _guard) = require_db!();
        let repo = PgRepresentativeRepository::new(pool);
        let term = Term::parse("2026-2030").unwrap();
        repo.upsert_all(&[rep("2026-2030", "Ana Paola")], at(7))
            .await
            .unwrap();
        let mut changed = rep("2026-2030", "Ana Paola");
        changed.party = Some("Otro".into());
        repo.upsert_all(&[changed], at(9)).await.unwrap();
        let listed = repo.list(Some(&term)).await.unwrap();
        assert_eq!(listed[0].party.as_deref(), Some("Otro"));
        assert_eq!(listed[0].constituency.as_deref(), Some("NARIÑO"));
        assert_eq!(repo.last_updated(&term).await.unwrap(), Some(at(9)));
    }

    #[tokio::test]
    async fn list_filters_and_orders() {
        let (pool, _guard) = require_db!();
        let repo = PgRepresentativeRepository::new(pool);
        repo.upsert_all(
            &[
                rep("2018-2022", "Zeta Old"),
                rep("2026-2030", "Bravo"),
                rep("2026-2030", "Alfa"),
            ],
            at(7),
        )
        .await
        .unwrap();
        let current = repo
            .list(Some(&Term::parse("2026-2030").unwrap()))
            .await
            .unwrap();
        let names: Vec<&str> = current.iter().map(|r| r.full_name.as_str()).collect();
        assert_eq!(names, vec!["Alfa", "Bravo"]);
        let all = repo.list(None).await.unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[2].full_name, "Zeta Old");
        assert_eq!(all[0].source, SourceKind::DatosGovCo);
    }

    #[tokio::test]
    async fn last_updated_none_for_empty_term() {
        let (pool, _guard) = require_db!();
        let repo = PgRepresentativeRepository::new(pool);
        assert_eq!(
            repo.last_updated(&Term::parse("2026-2030").unwrap())
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn advisory_lock_is_exclusive() {
        let (pool, _guard) = require_db!();
        let repo = PgRepresentativeRepository::new(pool);
        let first = repo.try_sync_lock().await.unwrap();
        assert!(first.is_some());
        assert!(repo.try_sync_lock().await.unwrap().is_none());
        drop(first);
        assert!(repo.try_sync_lock().await.unwrap().is_some());
    }

    #[tokio::test]
    async fn source_check_rejects_senado_for_representatives() {
        let (pool, _guard) = require_db!();
        let result = sqlx::query(
            "INSERT INTO representatives (id, term, normalized_name, full_name, gender, source, updated_at)
             VALUES ('x', '2026-2030', 'x', 'x', 'unknown', 'senado', now())",
        )
        .execute(&pool)
        .await;
        assert!(result.is_err());
    }

    #[test]
    fn source_and_gender_helpers() {
        assert_eq!(source_from("congreso_visible"), SourceKind::CongresoVisible);
        assert_eq!(source_from("datos_gov_co"), SourceKind::DatosGovCo);
        assert_eq!(gender_from("male"), Gender::Male);
    }
}
