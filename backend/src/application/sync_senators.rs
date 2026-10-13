use chrono::{DateTime, Duration, Utc};
use tracing::{error, info, warn};

use super::errors::{RepositoryError, SourceError, SyncError};
use super::ports::{Clock, SenatorRepository, SenatorSource};
use crate::domain::senator::{Senator, Term};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoricalOutcome {
    Skipped,
    Loaded(usize),
}

pub struct SyncSenators<R, C> {
    repo: R,
    clock: C,
}

impl<R: SenatorRepository, C: Clock> SyncSenators<R, C> {
    pub fn new(repo: R, clock: C) -> Self {
        Self { repo, clock }
    }

    pub async fn load_historical<S: SenatorSource>(
        &self,
        term: &Term,
        dataset: &S,
    ) -> Result<HistoricalOutcome, SyncError> {
        if self.repo.has_term(term).await? {
            return Ok(HistoricalOutcome::Skipped);
        }
        let senators = match fetch_non_empty(dataset, term).await {
            Ok(senators) => senators,
            Err(e) => {
                error!(term = term.as_str(), source = ?dataset.kind(), error = %e, "historical load failed");
                return Err(SyncError::AllSourcesFailed);
            }
        };
        let count = self.repo.upsert_all(&senators, self.clock.now()).await?;
        info!(term = term.as_str(), count, "historical load stored");
        Ok(HistoricalOutcome::Loaded(count))
    }

    pub async fn refresh_current<P, F>(
        &self,
        term: &Term,
        primary: &P,
        fallback: Option<&F>,
    ) -> Result<usize, SyncError>
    where
        P: SenatorSource,
        F: SenatorSource,
    {
        let senators = match fetch_non_empty(primary, term).await {
            Ok(senators) => senators,
            Err(primary_error) => {
                warn!(term = term.as_str(), source = ?primary.kind(), error = %primary_error, "primary source failed");
                let Some(fallback) = fallback else {
                    error!(
                        term = term.as_str(),
                        "no fallback source; run failed, stored data kept"
                    );
                    return Err(SyncError::AllSourcesFailed);
                };
                match fetch_non_empty(fallback, term).await {
                    Ok(senators) => {
                        info!(term = term.as_str(), source = ?fallback.kind(), "fallback source used");
                        senators
                    }
                    Err(fallback_error) => {
                        error!(term = term.as_str(), source = ?fallback.kind(), error = %fallback_error, "fallback source failed; run failed, stored data kept");
                        return Err(SyncError::AllSourcesFailed);
                    }
                }
            }
        };
        let count = self.repo.upsert_all(&senators, self.clock.now()).await?;
        info!(term = term.as_str(), count, "current refresh stored");
        Ok(count)
    }

    pub fn is_stale(&self, last_updated: Option<DateTime<Utc>>) -> bool {
        is_stale(self.clock.now(), last_updated)
    }

    pub async fn last_updated(
        &self,
        term: &Term,
    ) -> Result<Option<DateTime<Utc>>, RepositoryError> {
        self.repo.last_updated(term).await
    }

    pub async fn try_sync_lock(&self) -> Result<Option<R::Lock>, RepositoryError> {
        self.repo.try_sync_lock().await
    }

    pub fn clock(&self) -> &C {
        &self.clock
    }
}

async fn fetch_non_empty<S: SenatorSource>(
    source: &S,
    term: &Term,
) -> Result<Vec<Senator>, SourceError> {
    let senators = source.fetch(term).await?;
    if senators.is_empty() {
        Err(SourceError::Empty)
    } else {
        Ok(senators)
    }
}

pub fn is_stale(now: DateTime<Utc>, last_updated: Option<DateTime<Utc>>) -> bool {
    match last_updated {
        None => true,
        Some(at) => now - at > Duration::hours(24),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use chrono::TimeZone;

    use super::*;
    use crate::domain::senator::SourceKind;

    struct FakeSource {
        kind: SourceKind,
        result: Result<Vec<Senator>, SourceError>,
        calls: AtomicUsize,
    }

    impl FakeSource {
        fn ok(kind: SourceKind, names: &[&str]) -> Self {
            let term = Term::parse("2026-2030").unwrap();
            let senators = names
                .iter()
                .map(|n| Senator::new(term.clone(), n.to_string(), kind))
                .collect();
            Self {
                kind,
                result: Ok(senators),
                calls: AtomicUsize::new(0),
            }
        }

        fn err(kind: SourceKind, error: SourceError) -> Self {
            Self {
                kind,
                result: Err(error),
                calls: AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl SenatorSource for FakeSource {
        fn kind(&self) -> SourceKind {
            self.kind
        }

        async fn fetch(&self, _term: &Term) -> Result<Vec<Senator>, SourceError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.result.clone()
        }
    }

    #[derive(Default)]
    struct FakeRepo {
        has_term: bool,
        last_updated: Option<DateTime<Utc>>,
        fail_upsert: bool,
        upserted: Mutex<Vec<usize>>,
    }

    impl SenatorRepository for FakeRepo {
        type Lock = ();

        async fn has_term(&self, _term: &Term) -> Result<bool, RepositoryError> {
            Ok(self.has_term)
        }

        async fn last_updated(
            &self,
            _term: &Term,
        ) -> Result<Option<DateTime<Utc>>, RepositoryError> {
            Ok(self.last_updated)
        }

        async fn upsert_all(
            &self,
            senators: &[Senator],
            _now: DateTime<Utc>,
        ) -> Result<usize, RepositoryError> {
            if self.fail_upsert {
                return Err(RepositoryError("boom".into()));
            }
            self.upserted.lock().unwrap().push(senators.len());
            Ok(senators.len())
        }

        async fn list(&self, _term: Option<&Term>) -> Result<Vec<Senator>, RepositoryError> {
            Ok(vec![])
        }

        async fn try_sync_lock(&self) -> Result<Option<()>, RepositoryError> {
            Ok(Some(()))
        }
    }

    struct FixedClock(DateTime<Utc>);

    impl Clock for FixedClock {
        fn now(&self) -> DateTime<Utc> {
            self.0
        }
    }

    fn clock() -> FixedClock {
        FixedClock(Utc.with_ymd_and_hms(2026, 10, 11, 7, 0, 0).unwrap())
    }

    fn term() -> Term {
        Term::parse("2026-2030").unwrap()
    }

    fn no_fallback() -> Option<&'static FakeSource> {
        None
    }

    #[tokio::test]
    async fn primary_success_never_calls_fallback() {
        let primary = FakeSource::ok(SourceKind::Senado, &["Ana Paola", "Luis Pérez"]);
        let fallback = FakeSource::ok(SourceKind::Registraduria, &["Otro"]);
        let sync = SyncSenators::new(FakeRepo::default(), clock());
        let count = sync
            .refresh_current(&term(), &primary, Some(&fallback))
            .await
            .unwrap();
        assert_eq!(count, 2);
        assert_eq!(fallback.calls(), 0);
    }

    #[tokio::test]
    async fn primary_fails_fallback_succeeds() {
        let primary = FakeSource::err(SourceKind::Senado, SourceError::Network);
        let fallback = FakeSource::ok(SourceKind::Registraduria, &["Otro"]);
        let sync = SyncSenators::new(FakeRepo::default(), clock());
        let count = sync
            .refresh_current(&term(), &primary, Some(&fallback))
            .await
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(fallback.calls(), 1);
    }

    #[tokio::test]
    async fn both_sources_fail_leaves_data_untouched() {
        let primary = FakeSource::err(SourceKind::Senado, SourceError::UnexpectedStatus(500));
        let fallback = FakeSource::err(SourceKind::Registraduria, SourceError::Network);
        let repo = FakeRepo::default();
        let sync = SyncSenators::new(repo, clock());
        let result = sync
            .refresh_current(&term(), &primary, Some(&fallback))
            .await;
        assert_eq!(result, Err(SyncError::AllSourcesFailed));
        assert!(sync.repo.upserted.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn primary_empty_counts_as_failure() {
        let primary = FakeSource::ok(SourceKind::Senado, &[]);
        let fallback = FakeSource::ok(SourceKind::Registraduria, &["Otro"]);
        let sync = SyncSenators::new(FakeRepo::default(), clock());
        let count = sync
            .refresh_current(&term(), &primary, Some(&fallback))
            .await
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(fallback.calls(), 1);
    }

    #[tokio::test]
    async fn no_fallback_and_primary_fails_is_all_sources_failed() {
        let primary = FakeSource::err(SourceKind::Senado, SourceError::Parse("x".into()));
        let sync = SyncSenators::new(FakeRepo::default(), clock());
        let result = sync.refresh_current(&term(), &primary, no_fallback()).await;
        assert_eq!(result, Err(SyncError::AllSourcesFailed));
    }

    #[tokio::test]
    async fn repository_failure_is_reported_and_nothing_stored() {
        let primary = FakeSource::ok(SourceKind::Senado, &["Ana"]);
        let repo = FakeRepo {
            fail_upsert: true,
            ..Default::default()
        };
        let sync = SyncSenators::new(repo, clock());
        let result = sync.refresh_current(&term(), &primary, no_fallback()).await;
        assert_eq!(
            result,
            Err(SyncError::Repository(RepositoryError("boom".into())))
        );
    }

    #[tokio::test]
    async fn historical_skipped_when_present() {
        let dataset = FakeSource::ok(SourceKind::DatosGovCo, &["Ana"]);
        let repo = FakeRepo {
            has_term: true,
            ..Default::default()
        };
        let sync = SyncSenators::new(repo, clock());
        let outcome = sync.load_historical(&term(), &dataset).await.unwrap();
        assert_eq!(outcome, HistoricalOutcome::Skipped);
        assert_eq!(dataset.calls(), 0);
    }

    #[tokio::test]
    async fn historical_loads_when_absent() {
        let dataset = FakeSource::ok(SourceKind::DatosGovCo, &["Ana", "Luis"]);
        let sync = SyncSenators::new(FakeRepo::default(), clock());
        let outcome = sync.load_historical(&term(), &dataset).await.unwrap();
        assert_eq!(outcome, HistoricalOutcome::Loaded(2));
    }

    #[tokio::test]
    async fn historical_failure_stores_nothing() {
        let dataset = FakeSource::err(SourceKind::DatosGovCo, SourceError::Network);
        let sync = SyncSenators::new(FakeRepo::default(), clock());
        let result = sync.load_historical(&term(), &dataset).await;
        assert_eq!(result, Err(SyncError::AllSourcesFailed));
        assert!(sync.repo.upserted.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn historical_repository_error_is_reported() {
        let dataset = FakeSource::ok(SourceKind::DatosGovCo, &["Ana"]);
        let repo = FakeRepo {
            fail_upsert: true,
            ..Default::default()
        };
        let sync = SyncSenators::new(repo, clock());
        let result = sync.load_historical(&term(), &dataset).await;
        assert!(matches!(result, Err(SyncError::Repository(_))));
    }

    #[test]
    fn staleness_rule() {
        let now = Utc.with_ymd_and_hms(2026, 10, 11, 7, 0, 0).unwrap();
        assert!(is_stale(now, None));
        assert!(!is_stale(now, Some(now - Duration::hours(23))));
        assert!(is_stale(now, Some(now - Duration::hours(25))));
    }

    #[tokio::test]
    async fn sync_helpers_delegate_to_repo_and_clock() {
        let repo = FakeRepo {
            last_updated: Some(Utc::now()),
            ..Default::default()
        };
        let sync = SyncSenators::new(repo, clock());
        assert!(sync.last_updated(&term()).await.unwrap().is_some());
        assert!(sync.try_sync_lock().await.unwrap().is_some());
        assert!(sync.is_stale(None));
        assert_eq!(chrono::Datelike::year(&sync.clock().now()), 2026);
    }
}
