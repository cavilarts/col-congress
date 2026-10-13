pub mod representative_repository;
pub mod senator_repository;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

pub use representative_repository::PgRepresentativeRepository;
pub use senator_repository::{PgSenatorRepository, SyncLock};

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::OnceLock;

    use sqlx::PgPool;
    use tokio::sync::{Mutex, MutexGuard};

    use super::MIGRATOR;

    static DB_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    pub async fn test_pool() -> Option<(PgPool, MutexGuard<'static, ()>)> {
        let url = std::env::var("TEST_DATABASE_URL").ok()?;
        let guard = DB_LOCK.get_or_init(|| Mutex::new(())).lock().await;
        let pool = PgPool::connect(&url)
            .await
            .expect("TEST_DATABASE_URL must be reachable");
        MIGRATOR.run(&pool).await.expect("migrations apply");
        sqlx::query("TRUNCATE senators, representatives")
            .execute(&pool)
            .await
            .unwrap();
        Some((pool, guard))
    }
}
