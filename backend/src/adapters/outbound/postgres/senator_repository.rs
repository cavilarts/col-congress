use chrono::{DateTime, NaiveDate, Utc};
use sqlx::pool::PoolConnection;
use sqlx::postgres::PgRow;
use sqlx::{PgConnection, PgPool, Postgres, Row, Transaction};
use tracing::error;

use crate::application::errors::RepositoryError;
use crate::application::ports::SenatorRepository;
use crate::domain::senator::{Gender, Senator, SenatorId, SourceKind, Term};

pub const SENATORS_SYNC_LOCK_KEY: i64 = 0x5E4A_7A01;

pub struct SyncLock {
    _connection: PgConnection,
}

impl SyncLock {
    pub(crate) fn new(connection: PgConnection) -> Self {
        Self {
            _connection: connection,
        }
    }
}

#[derive(Clone)]
pub struct PgSenatorRepository {
    pool: PgPool,
}

impl PgSenatorRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

pub(crate) fn storage(error: sqlx::Error) -> RepositoryError {
    error!(error = %error, "senators storage operation failed");
    let category = match error {
        sqlx::Error::Io(_) | sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed => "connection",
        sqlx::Error::Database(_) => "query",
        _ => "storage",
    };
    RepositoryError(format!("{category} error"))
}

const INSERT: &str = "INSERT INTO senators (
    id, term, normalized_name, full_name, gender, birth_date, department_of_birth,
    city_of_birth, party, votes, constitutional_commission, legal_commission, email,
    phone, phone_extension, website, twitter, facebook, source, updated_at
) VALUES (
    $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20
) ON CONFLICT (id) DO UPDATE SET
    term = EXCLUDED.term,
    normalized_name = EXCLUDED.normalized_name,
    full_name = EXCLUDED.full_name,
    gender = EXCLUDED.gender,
    birth_date = EXCLUDED.birth_date,
    department_of_birth = EXCLUDED.department_of_birth,
    city_of_birth = EXCLUDED.city_of_birth,
    party = EXCLUDED.party,
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
    senator: &Senator,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(INSERT)
        .bind(senator.id.as_str())
        .bind(senator.term.as_str())
        .bind(&senator.normalized_name)
        .bind(&senator.full_name)
        .bind(senator.gender.as_str())
        .bind(senator.birth_date)
        .bind(&senator.department_of_birth)
        .bind(&senator.city_of_birth)
        .bind(&senator.party)
        .bind(senator.votes.map(|v| v as i64))
        .bind(&senator.constitutional_commission)
        .bind(&senator.legal_commission)
        .bind(&senator.email)
        .bind(&senator.phone)
        .bind(&senator.phone_extension)
        .bind(&senator.website)
        .bind(&senator.twitter)
        .bind(&senator.facebook)
        .bind(senator.source.as_str())
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
        "senado" => SourceKind::Senado,
        _ => SourceKind::Registraduria,
    }
}

fn map_row(row: &PgRow) -> Result<Senator, sqlx::Error> {
    let term_text: String = row.try_get("term")?;
    let term =
        Term::parse(&term_text).ok_or_else(|| sqlx::Error::Decode("invalid term in row".into()))?;
    let id: String = row.try_get("id")?;
    let gender: String = row.try_get("gender")?;
    let source: String = row.try_get("source")?;
    let birth_date: Option<NaiveDate> = row.try_get("birth_date")?;
    let votes: Option<i64> = row.try_get("votes")?;
    Ok(Senator {
        id: SenatorId::from_stored(id),
        term,
        full_name: row.try_get("full_name")?,
        normalized_name: row.try_get("normalized_name")?,
        gender: gender_from(&gender),
        birth_date,
        department_of_birth: row.try_get("department_of_birth")?,
        city_of_birth: row.try_get("city_of_birth")?,
        party: row.try_get("party")?,
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

impl SenatorRepository for PgSenatorRepository {
    type Lock = SyncLock;

    async fn has_term(&self, term: &Term) -> Result<bool, RepositoryError> {
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM senators WHERE term = $1)")
            .bind(term.as_str())
            .fetch_one(&self.pool)
            .await
            .map_err(storage)
    }

    async fn last_updated(&self, term: &Term) -> Result<Option<DateTime<Utc>>, RepositoryError> {
        sqlx::query_scalar("SELECT MAX(updated_at) FROM senators WHERE term = $1")
            .bind(term.as_str())
            .fetch_one(&self.pool)
            .await
            .map_err(storage)
    }

    async fn upsert_all(
        &self,
        senators: &[Senator],
        now: DateTime<Utc>,
    ) -> Result<usize, RepositoryError> {
        let mut tx = self.pool.begin().await.map_err(storage)?;
        for senator in senators {
            insert(&mut tx, senator, now).await.map_err(storage)?;
        }
        tx.commit().await.map_err(storage)?;
        Ok(senators.len())
    }

    async fn list(&self, term: Option<&Term>) -> Result<Vec<Senator>, RepositoryError> {
        const SELECT: &str = "SELECT * FROM senators";
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
            .bind(SENATORS_SYNC_LOCK_KEY)
            .fetch_one(&mut *connection)
            .await
            .map_err(storage)?;
        if !acquired {
            return Ok(None);
        }
        Ok(Some(SyncLock {
            _connection: connection.detach(),
        }))
    }
}

#[cfg(test)]
mod integration {
    use chrono::{Duration, TimeZone};

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

    fn senator(term: &str, name: &str) -> Senator {
        let mut s = Senator::new(
            Term::parse(term).unwrap(),
            name.into(),
            SourceKind::DatosGovCo,
        );
        s.votes = Some(1000);
        s.birth_date = NaiveDate::from_ymd_opt(1970, 1, 2);
        s.party = Some("MIRA".into());
        s
    }

    fn at(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 11, hour, 0, 0).unwrap()
    }

    #[tokio::test]
    async fn migrations_create_the_table() {
        let (pool, _guard) = require_db!();
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'senators')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(exists);
    }

    #[tokio::test]
    async fn upsert_twice_creates_no_duplicates() {
        let (pool, _guard) = require_db!();
        let repo = PgSenatorRepository::new(pool.clone());
        let batch = vec![
            senator("2026-2030", "Ana Paola"),
            senator("2026-2030", "Luis Pérez"),
        ];
        repo.upsert_all(&batch, at(7)).await.unwrap();
        let written = repo.upsert_all(&batch, at(8)).await.unwrap();
        assert_eq!(written, 2);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM senators")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn upsert_updates_updated_at_and_fields() {
        let (pool, _guard) = require_db!();
        let repo = PgSenatorRepository::new(pool.clone());
        repo.upsert_all(&[senator("2026-2030", "Ana Paola")], at(7))
            .await
            .unwrap();
        let mut changed = senator("2026-2030", "Ana Paola");
        changed.party = Some("Otro".into());
        repo.upsert_all(&[changed], at(9)).await.unwrap();
        let listed = repo
            .list(Some(&Term::parse("2026-2030").unwrap()))
            .await
            .unwrap();
        assert_eq!(listed[0].party.as_deref(), Some("Otro"));
        assert_eq!(
            repo.last_updated(&Term::parse("2026-2030").unwrap())
                .await
                .unwrap(),
            Some(at(9))
        );
    }

    #[tokio::test]
    async fn list_filters_by_term_and_orders_by_term_desc_then_name() {
        let (pool, _guard) = require_db!();
        let repo = PgSenatorRepository::new(pool.clone());
        repo.upsert_all(
            &[
                senator("2018-2022", "Zeta Old"),
                senator("2026-2030", "Bravo New"),
                senator("2026-2030", "Alfa New"),
            ],
            at(7),
        )
        .await
        .unwrap();
        let current = repo
            .list(Some(&Term::parse("2026-2030").unwrap()))
            .await
            .unwrap();
        let names: Vec<&str> = current.iter().map(|s| s.full_name.as_str()).collect();
        assert_eq!(names, vec!["Alfa New", "Bravo New"]);

        let all = repo.list(None).await.unwrap();
        let order: Vec<&str> = all.iter().map(|s| s.full_name.as_str()).collect();
        assert_eq!(order, vec!["Alfa New", "Bravo New", "Zeta Old"]);
        assert_eq!(all[0].gender, Gender::Unknown);
        assert_eq!(all[0].votes, Some(1000));
        assert_eq!(all[0].birth_date, NaiveDate::from_ymd_opt(1970, 1, 2));
    }

    #[tokio::test]
    async fn last_updated_and_has_term_for_empty_term() {
        let (pool, _guard) = require_db!();
        let repo = PgSenatorRepository::new(pool);
        let term = Term::parse("2018-2022").unwrap();
        assert_eq!(repo.last_updated(&term).await.unwrap(), None);
        assert!(!repo.has_term(&term).await.unwrap());
        repo.upsert_all(&[senator("2018-2022", "Ana")], at(7) + Duration::hours(1))
            .await
            .unwrap();
        assert!(repo.has_term(&term).await.unwrap());
    }

    #[tokio::test]
    async fn advisory_lock_blocks_second_holder_until_released() {
        let (pool, _guard) = require_db!();
        let repo = PgSenatorRepository::new(pool);
        let first = repo.try_sync_lock().await.unwrap();
        assert!(first.is_some());
        let second = repo.try_sync_lock().await.unwrap();
        assert!(second.is_none());
        drop(first);
        let third = repo.try_sync_lock().await.unwrap();
        assert!(third.is_some());
    }

    #[tokio::test]
    async fn source_check_rejects_unknown_values() {
        let (pool, _guard) = require_db!();
        let result = sqlx::query(
            "INSERT INTO senators (id, term, normalized_name, full_name, gender, source, updated_at)
             VALUES ('x', '2026-2030', 'x', 'x', 'unknown', 'otra', now())",
        )
        .execute(&pool)
        .await;
        assert!(result.is_err());
    }

    #[test]
    fn storage_errors_are_generic() {
        assert_eq!(storage(sqlx::Error::PoolTimedOut).0, "connection error");
        assert_eq!(storage(sqlx::Error::RowNotFound).0, "storage error");
    }

    #[test]
    fn row_helpers_map_known_values() {
        assert_eq!(gender_from("female"), Gender::Female);
        assert_eq!(gender_from("zzz"), Gender::Unknown);
        assert_eq!(source_from("senado"), SourceKind::Senado);
        assert_eq!(source_from("registraduria"), SourceKind::Registraduria);
    }
}
