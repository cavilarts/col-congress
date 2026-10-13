use std::future::Future;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, NaiveTime, Utc};
use chrono_tz::{America::Bogota, Tz};
use tokio::time::sleep;
use tracing::{error, info};

use crate::application::errors::SyncError;
use crate::application::ports::{
    Clock, RepresentativeRepository, RepresentativeSource, SenatorRepository, SenatorSource,
};
use crate::application::sync_representatives::SyncRepresentatives;
use crate::application::sync_senators::{HistoricalOutcome, SyncSenators};
use crate::domain::senator::Term;

pub const WINDOW_START: NaiveTime = NaiveTime::from_hms_opt(2, 0, 0).unwrap();
pub const WINDOW_END: NaiveTime = NaiveTime::from_hms_opt(23, 59, 59).unwrap();
pub const WINDOW_SECONDS: u32 = 3 * 60 * 60;

pub fn next_run_after(now: DateTime<Utc>, offset_seconds: u32) -> DateTime<Utc> {
    let offset = Duration::seconds(i64::from(offset_seconds % WINDOW_SECONDS));
    let mut day = now.with_timezone(&Bogota).date_naive();
    for _ in 0..3 {
        if let Some(local_start) = day
            .and_time(WINDOW_START)
            .and_local_timezone(Bogota)
            .single()
        {
            let candidate = local_start.with_timezone(&Utc) + offset;
            if candidate > now {
                return candidate;
            }
        }
        match day.succ_opt() {
            Some(next) => day = next,
            None => break,
        }
    }
    now
}

pub fn bogota_local(now: DateTime<Utc>) -> DateTime<Tz> {
    now.with_timezone(&Bogota)
}

pub struct SenatorsJob<R, C, D, P, F> {
    pub sync: SyncSenators<R, C>,
    pub historical: Term,
    pub current: Term,
    pub dataset: D,
    pub primary: P,
    pub fallback: Option<F>,
}

impl<R, C, D, P, F> SenatorsJob<R, C, D, P, F>
where
    R: SenatorRepository,
    C: Clock,
    D: SenatorSource,
    P: SenatorSource,
    F: SenatorSource,
{
    pub async fn run_startup(&self) {
        match self
            .sync
            .load_historical(&self.historical, &self.dataset)
            .await
        {
            Ok(HistoricalOutcome::Skipped) => info!("historical term already loaded"),
            Ok(HistoricalOutcome::Loaded(count)) => info!(count, "historical term loaded"),
            Err(e) => error!(error = %e, "historical load failed"),
        }
        match self.sync.last_updated(&self.current).await {
            Ok(last) if self.sync.is_stale(last) => {
                if let Err(e) = self.run_once().await {
                    error!(error = %e, "startup refresh failed");
                }
            }
            Ok(_) => info!("current term data is fresh; no startup refresh"),
            Err(e) => error!(error = %e, "could not read last update time"),
        }
    }

    pub async fn run_once(&self) -> Result<Option<usize>, SyncError> {
        let Some(_lock) = self.sync.try_sync_lock().await? else {
            info!("sync lock held by another instance; run skipped");
            return Ok(None);
        };
        self.sync
            .refresh_current(&self.current, &self.primary, self.fallback.as_ref())
            .await
            .map(Some)
    }

    pub async fn run_daily(&self) {
        run_daily(self.sync.clock(), || async {
            match self.run_once().await {
                Ok(Some(count)) => info!(count, "daily senators sync finished"),
                Ok(None) => {}
                Err(e) => error!(error = %e, "daily senators sync failed"),
            }
        })
        .await
    }
}

pub struct RepresentativesJob<R, C, P, F> {
    pub sync: SyncRepresentatives<R, C>,
    pub current: Term,
    pub primary: P,
    pub fallback: Option<F>,
}

impl<R, C, P, F> RepresentativesJob<R, C, P, F>
where
    R: RepresentativeRepository,
    C: Clock,
    P: RepresentativeSource,
    F: RepresentativeSource,
{
    pub async fn run_startup(&self) {
        match self.sync.last_updated(&self.current).await {
            Ok(last) if self.sync.is_stale(last) => {
                if let Err(e) = self.run_once().await {
                    error!(error = %e, "representatives startup refresh failed");
                }
            }
            Ok(_) => info!("current representatives data is fresh; no startup refresh"),
            Err(e) => error!(error = %e, "could not read representatives last update time"),
        }
    }

    pub async fn run_once(&self) -> Result<Option<usize>, SyncError> {
        let Some(_lock) = self.sync.try_sync_lock().await? else {
            info!("representatives sync lock held by another instance; run skipped");
            return Ok(None);
        };
        self.sync
            .refresh_current(&self.current, &self.primary, self.fallback.as_ref())
            .await
            .map(Some)
    }

    pub async fn run_daily(&self) {
        run_daily(self.sync.clock(), || async {
            match self.run_once().await {
                Ok(Some(count)) => info!(count, "daily representatives sync finished"),
                Ok(None) => {}
                Err(e) => error!(error = %e, "daily representatives sync failed"),
            }
        })
        .await
    }
}

pub async fn run_daily<C, F, Fut>(clock: &C, mut run: F)
where
    C: Clock,
    F: FnMut() -> Fut,
    Fut: Future<Output = ()>,
{
    let mut cursor = clock.now();
    loop {
        let offset: u32 = rand::random_range(0..WINDOW_SECONDS);
        let next = next_run_after(cursor, offset);
        let wait = (next - clock.now()).to_std().unwrap_or(StdDuration::ZERO);
        info!(next_run = %next, "daily sync scheduled");
        sleep(wait).await;
        run().await;
        cursor = end_of_local_day(next);
    }
}

pub fn end_of_local_day(instant: DateTime<Utc>) -> DateTime<Utc> {
    let day = instant.with_timezone(&Bogota).date_naive();
    day.and_time(WINDOW_END)
        .and_local_timezone(Bogota)
        .single()
        .map(|local| local.with_timezone(&Utc))
        .unwrap_or(instant)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn bogota(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Bogota
            .with_ymd_and_hms(y, mo, d, h, mi, s)
            .single()
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn before_window_runs_today_at_offset() {
        let now = bogota(2026, 10, 11, 1, 0, 0);
        assert_eq!(next_run_after(now, 0), bogota(2026, 10, 11, 2, 0, 0));
        assert_eq!(next_run_after(now, 3600), bogota(2026, 10, 11, 3, 0, 0));
    }

    #[test]
    fn inside_window_after_candidate_runs_tomorrow() {
        let now = bogota(2026, 10, 11, 4, 59, 59);
        assert_eq!(next_run_after(now, 0), bogota(2026, 10, 12, 2, 0, 0));
    }

    #[test]
    fn inside_window_before_candidate_runs_today() {
        let now = bogota(2026, 10, 11, 2, 30, 0);
        assert_eq!(next_run_after(now, 3600), bogota(2026, 10, 11, 3, 0, 0));
    }

    #[test]
    fn exactly_at_candidate_is_strictly_after() {
        let now = bogota(2026, 10, 11, 3, 0, 0);
        assert_eq!(next_run_after(now, 3600), bogota(2026, 10, 12, 3, 0, 0));
    }

    #[test]
    fn exactly_at_window_end_runs_tomorrow() {
        let now = bogota(2026, 10, 11, 5, 0, 0);
        assert_eq!(next_run_after(now, 0), bogota(2026, 10, 12, 2, 0, 0));
    }

    #[test]
    fn crosses_midnight() {
        let now = bogota(2026, 10, 11, 23, 30, 0);
        assert_eq!(next_run_after(now, 1800), bogota(2026, 10, 12, 2, 30, 0));
    }

    #[test]
    fn offset_is_wrapped_into_window() {
        let now = bogota(2026, 10, 11, 1, 0, 0);
        assert_eq!(
            next_run_after(now, WINDOW_SECONDS + 60),
            bogota(2026, 10, 11, 2, 1, 0)
        );
    }

    #[test]
    fn no_daylight_saving_in_colombia() {
        let june = Utc.with_ymd_and_hms(2026, 6, 15, 0, 0, 0).unwrap();
        assert_eq!(
            next_run_after(june, 0),
            Utc.with_ymd_and_hms(2026, 6, 15, 7, 0, 0).unwrap()
        );
        assert_eq!(bogota_local(june).format("%H:%M").to_string(), "19:00");
    }
}

#[cfg(test)]
mod job_tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use chrono::TimeZone;

    use super::*;
    use crate::application::errors::{RepositoryError, SourceError};
    use crate::domain::senator::{Senator, SourceKind};

    struct Repo {
        lock_available: bool,
        last_updated: Option<DateTime<Utc>>,
        has_term: bool,
        stored: Mutex<usize>,
    }

    impl Repo {
        fn new(lock_available: bool, last_updated: Option<DateTime<Utc>>) -> Self {
            Self {
                lock_available,
                last_updated,
                has_term: true,
                stored: Mutex::new(0),
            }
        }
    }

    impl SenatorRepository for Repo {
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
            *self.stored.lock().unwrap() += senators.len();
            Ok(senators.len())
        }

        async fn list(&self, _term: Option<&Term>) -> Result<Vec<Senator>, RepositoryError> {
            Ok(vec![])
        }

        async fn try_sync_lock(&self) -> Result<Option<()>, RepositoryError> {
            Ok(if self.lock_available { Some(()) } else { None })
        }
    }

    struct Clock0(DateTime<Utc>);

    impl Clock for Clock0 {
        fn now(&self) -> DateTime<Utc> {
            self.0
        }
    }

    struct Source {
        ok: bool,
        calls: AtomicUsize,
    }

    impl SenatorSource for Source {
        fn kind(&self) -> SourceKind {
            SourceKind::Senado
        }

        async fn fetch(&self, term: &Term) -> Result<Vec<Senator>, SourceError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.ok {
                Ok(vec![Senator::new(
                    term.clone(),
                    "Ana Paola".into(),
                    SourceKind::Senado,
                )])
            } else {
                Err(SourceError::Network)
            }
        }
    }

    fn source(ok: bool) -> Source {
        Source {
            ok,
            calls: AtomicUsize::new(0),
        }
    }

    fn job(
        repo: Repo,
        now: DateTime<Utc>,
        primary_ok: bool,
    ) -> SenatorsJob<Repo, Clock0, Source, Source, Source> {
        SenatorsJob {
            sync: SyncSenators::new(repo, Clock0(now)),
            historical: Term::parse("2018-2022").unwrap(),
            current: Term::parse("2026-2030").unwrap(),
            dataset: source(true),
            primary: source(primary_ok),
            fallback: None,
        }
    }

    fn at(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 11, hour, 0, 0).unwrap()
    }

    #[tokio::test]
    async fn run_once_stores_data_when_lock_is_free() {
        let job = job(Repo::new(true, None), at(7), true);
        assert_eq!(job.run_once().await.unwrap(), Some(1));
        assert_eq!(job.primary.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn run_once_skips_when_lock_is_busy() {
        let job = job(Repo::new(false, None), at(7), true);
        assert_eq!(job.run_once().await.unwrap(), None);
        assert_eq!(job.primary.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn run_once_reports_source_failure() {
        let job = job(Repo::new(true, None), at(7), false);
        assert_eq!(job.run_once().await, Err(SyncError::AllSourcesFailed));
    }

    #[tokio::test]
    async fn startup_refreshes_when_stale() {
        let job = job(Repo::new(true, None), at(7), true);
        job.run_startup().await;
        assert_eq!(job.primary.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn startup_skips_refresh_when_fresh() {
        let job = job(Repo::new(true, Some(at(6))), at(7), true);
        job.run_startup().await;
        assert_eq!(job.primary.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn startup_tolerates_failures() {
        let job = job(Repo::new(true, None), at(7), false);
        job.run_startup().await;
        assert_eq!(job.primary.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn startup_loads_historical_when_missing() {
        let repo = Repo {
            has_term: false,
            ..Repo::new(true, Some(at(6)))
        };
        let job = job(repo, at(7), true);
        job.run_startup().await;
        assert_eq!(job.dataset.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn end_of_local_day_is_bogota_23_59_59() {
        let noon = at(12);
        let end = end_of_local_day(noon);
        assert_eq!(
            end,
            Bogota
                .with_ymd_and_hms(2026, 10, 11, 23, 59, 59)
                .unwrap()
                .with_timezone(&Utc)
        );
        assert!(next_run_after(end, 0) > end);
    }
}

#[cfg(test)]
mod representative_job_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use chrono::TimeZone;

    use super::*;
    use crate::application::errors::{RepositoryError, SourceError};
    use crate::domain::representative::Representative;
    use crate::domain::senator::SourceKind;

    struct Repo {
        lock_available: bool,
        last_updated: Option<DateTime<Utc>>,
    }

    impl RepresentativeRepository for Repo {
        type Lock = ();

        async fn last_updated(
            &self,
            _term: &Term,
        ) -> Result<Option<DateTime<Utc>>, RepositoryError> {
            Ok(self.last_updated)
        }

        async fn upsert_all(
            &self,
            reps: &[Representative],
            _now: DateTime<Utc>,
        ) -> Result<usize, RepositoryError> {
            Ok(reps.len())
        }

        async fn list(&self, _term: Option<&Term>) -> Result<Vec<Representative>, RepositoryError> {
            Ok(vec![])
        }

        async fn try_sync_lock(&self) -> Result<Option<()>, RepositoryError> {
            Ok(if self.lock_available { Some(()) } else { None })
        }
    }

    struct Clock0(DateTime<Utc>);

    impl Clock for Clock0 {
        fn now(&self) -> DateTime<Utc> {
            self.0
        }
    }

    struct Source {
        ok: bool,
        calls: AtomicUsize,
    }

    impl RepresentativeSource for Source {
        fn kind(&self) -> SourceKind {
            SourceKind::CongresoVisible
        }

        async fn fetch(&self, term: &Term) -> Result<Vec<Representative>, SourceError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.ok {
                Ok(vec![Representative::new(
                    term.clone(),
                    "Ana".into(),
                    SourceKind::CongresoVisible,
                )])
            } else {
                Err(SourceError::Network)
            }
        }
    }

    fn job(
        repo: Repo,
        now: DateTime<Utc>,
        ok: bool,
    ) -> RepresentativesJob<Repo, Clock0, Source, Source> {
        RepresentativesJob {
            sync: SyncRepresentatives::new(repo, Clock0(now)),
            current: Term::parse("2026-2030").unwrap(),
            primary: Source {
                ok,
                calls: AtomicUsize::new(0),
            },
            fallback: None,
        }
    }

    fn at(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 11, hour, 0, 0).unwrap()
    }

    #[tokio::test]
    async fn run_once_stores_when_lock_is_free() {
        let j = job(
            Repo {
                lock_available: true,
                last_updated: None,
            },
            at(7),
            true,
        );
        assert_eq!(j.run_once().await.unwrap(), Some(1));
    }

    #[tokio::test]
    async fn run_once_skips_when_lock_is_busy() {
        let j = job(
            Repo {
                lock_available: false,
                last_updated: None,
            },
            at(7),
            true,
        );
        assert_eq!(j.run_once().await.unwrap(), None);
        assert_eq!(j.primary.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn run_once_reports_failure() {
        let j = job(
            Repo {
                lock_available: true,
                last_updated: None,
            },
            at(7),
            false,
        );
        assert_eq!(j.run_once().await, Err(SyncError::AllSourcesFailed));
    }

    #[tokio::test]
    async fn startup_refreshes_only_when_stale() {
        let stale = job(
            Repo {
                lock_available: true,
                last_updated: None,
            },
            at(7),
            true,
        );
        stale.run_startup().await;
        assert_eq!(stale.primary.calls.load(Ordering::SeqCst), 1);

        let fresh = job(
            Repo {
                lock_available: true,
                last_updated: Some(at(6)),
            },
            at(7),
            true,
        );
        fresh.run_startup().await;
        assert_eq!(fresh.primary.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn startup_tolerates_failure() {
        let j = job(
            Repo {
                lock_available: true,
                last_updated: None,
            },
            at(7),
            false,
        );
        j.run_startup().await;
        assert_eq!(j.primary.calls.load(Ordering::SeqCst), 1);
    }
}

#[cfg(test)]
mod loop_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use chrono::TimeZone;

    use super::*;

    struct FixedClock(DateTime<Utc>);

    impl Clock for FixedClock {
        fn now(&self) -> DateTime<Utc> {
            self.0
        }
    }

    #[tokio::test(start_paused = true)]
    async fn daily_loop_runs_once_per_bogota_day() {
        let clock = FixedClock(Utc.with_ymd_and_hms(2026, 10, 11, 6, 0, 0).unwrap());
        let runs = AtomicUsize::new(0);
        let _ = tokio::time::timeout(
            StdDuration::from_secs(48 * 3600),
            run_daily(&clock, || async {
                runs.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .await;
        assert_eq!(runs.load(Ordering::SeqCst), 2);
    }
}
