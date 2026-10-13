use chrono::{DateTime, Utc};
use tracing::{error, info, warn};

use super::errors::{RepositoryError, SourceError, SyncError};
use super::ports::{Clock, RepresentativeRepository, RepresentativeSource};
use super::sync_senators::is_stale;
use crate::domain::representative::Representative;
use crate::domain::senator::Term;

pub const OFFICIAL_SEATS: usize = 183;

pub struct SyncRepresentatives<R, C> {
    repo: R,
    clock: C,
}

impl<R: RepresentativeRepository, C: Clock> SyncRepresentatives<R, C> {
    pub fn new(repo: R, clock: C) -> Self {
        Self { repo, clock }
    }

    pub async fn refresh_current<P, F>(
        &self,
        term: &Term,
        primary: &P,
        fallback: Option<&F>,
    ) -> Result<usize, SyncError>
    where
        P: RepresentativeSource,
        F: RepresentativeSource,
    {
        let representatives = match fetch_non_empty(primary, term).await {
            Ok(representatives) => representatives,
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
                    Ok(representatives) => {
                        info!(term = term.as_str(), source = ?fallback.kind(), "fallback source used");
                        representatives
                    }
                    Err(fallback_error) => {
                        error!(term = term.as_str(), source = ?fallback.kind(), error = %fallback_error, "fallback source failed; run failed, stored data kept");
                        return Err(SyncError::AllSourcesFailed);
                    }
                }
            }
        };
        if representatives.len() < OFFICIAL_SEATS {
            warn!(
                count = representatives.len(),
                expected = OFFICIAL_SEATS,
                "fewer representatives than official seats"
            );
        }
        let count = self
            .repo
            .upsert_all(&representatives, self.clock.now())
            .await?;
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

async fn fetch_non_empty<S: RepresentativeSource>(
    source: &S,
    term: &Term,
) -> Result<Vec<Representative>, SourceError> {
    let representatives = source.fetch(term).await?;
    if representatives.is_empty() {
        Err(SourceError::Empty)
    } else {
        Ok(representatives)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use chrono::TimeZone;

    use super::*;
    use crate::domain::senator::SourceKind;

    struct Source {
        result: Result<usize, SourceError>,
        calls: AtomicUsize,
    }

    impl Source {
        fn ok(count: usize) -> Self {
            Self {
                result: Ok(count),
                calls: AtomicUsize::new(0),
            }
        }

        fn err(error: SourceError) -> Self {
            Self {
                result: Err(error),
                calls: AtomicUsize::new(0),
            }
        }
    }

    impl RepresentativeSource for Source {
        fn kind(&self) -> SourceKind {
            SourceKind::DatosGovCo
        }

        async fn fetch(&self, term: &Term) -> Result<Vec<Representative>, SourceError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match &self.result {
                Ok(count) => Ok((0..*count)
                    .map(|i| {
                        Representative::new(
                            term.clone(),
                            format!("Persona {i}"),
                            SourceKind::DatosGovCo,
                        )
                    })
                    .collect()),
                Err(e) => Err(e.clone()),
            }
        }
    }

    #[derive(Default)]
    struct Repo {
        fail: bool,
        upserted: Mutex<Vec<usize>>,
    }

    impl RepresentativeRepository for Repo {
        type Lock = ();

        async fn last_updated(
            &self,
            _term: &Term,
        ) -> Result<Option<DateTime<Utc>>, RepositoryError> {
            Ok(None)
        }

        async fn upsert_all(
            &self,
            representatives: &[Representative],
            _now: DateTime<Utc>,
        ) -> Result<usize, RepositoryError> {
            if self.fail {
                return Err(RepositoryError("boom".into()));
            }
            self.upserted.lock().unwrap().push(representatives.len());
            Ok(representatives.len())
        }

        async fn list(&self, _term: Option<&Term>) -> Result<Vec<Representative>, RepositoryError> {
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

    fn sync(repo: Repo) -> SyncRepresentatives<Repo, FixedClock> {
        SyncRepresentatives::new(
            repo,
            FixedClock(Utc.with_ymd_and_hms(2026, 10, 11, 7, 0, 0).unwrap()),
        )
    }

    fn term() -> Term {
        Term::parse("2026-2030").unwrap()
    }

    #[tokio::test]
    async fn primary_success_skips_fallback() {
        let primary = Source::ok(3);
        let fallback = Source::ok(1);
        let s = sync(Repo::default());
        assert_eq!(
            s.refresh_current(&term(), &primary, Some(&fallback)).await,
            Ok(3)
        );
        assert_eq!(fallback.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn primary_failure_uses_fallback() {
        let primary = Source::err(SourceError::Network);
        let fallback = Source::ok(2);
        let s = sync(Repo::default());
        assert_eq!(
            s.refresh_current(&term(), &primary, Some(&fallback)).await,
            Ok(2)
        );
    }

    #[tokio::test]
    async fn both_failing_is_all_sources_failed_and_nothing_written() {
        let primary = Source::err(SourceError::Network);
        let fallback = Source::err(SourceError::UnexpectedStatus(500));
        let s = sync(Repo::default());
        assert_eq!(
            s.refresh_current(&term(), &primary, Some(&fallback)).await,
            Err(SyncError::AllSourcesFailed)
        );
        assert!(s.repo.upserted.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn empty_primary_is_a_failure() {
        let primary = Source::ok(0);
        let fallback = Source::ok(1);
        let s = sync(Repo::default());
        assert_eq!(
            s.refresh_current(&term(), &primary, Some(&fallback)).await,
            Ok(1)
        );
    }

    #[tokio::test]
    async fn no_fallback_and_primary_failure_fails_the_run() {
        let primary = Source::err(SourceError::Parse("x".into()));
        let s = sync(Repo::default());
        assert_eq!(
            s.refresh_current(&term(), &primary, None::<&Source>).await,
            Err(SyncError::AllSourcesFailed)
        );
    }

    #[tokio::test]
    async fn fewer_than_official_seats_is_stored_and_run_succeeds() {
        let primary = Source::ok(181);
        let s = sync(Repo::default());
        assert_eq!(
            s.refresh_current(&term(), &primary, None::<&Source>).await,
            Ok(181)
        );
    }

    #[tokio::test]
    async fn repository_failure_is_reported() {
        let primary = Source::ok(2);
        let s = sync(Repo {
            fail: true,
            ..Default::default()
        });
        assert_eq!(
            s.refresh_current(&term(), &primary, None::<&Source>).await,
            Err(SyncError::Repository(RepositoryError("boom".into())))
        );
    }

    #[tokio::test]
    async fn helpers_delegate() {
        let s = sync(Repo::default());
        assert!(s.is_stale(None));
        assert_eq!(s.last_updated(&term()).await.unwrap(), None);
        assert!(s.try_sync_lock().await.unwrap().is_some());
        assert_eq!(chrono::Datelike::year(&s.clock().now()), 2026);
    }
}
