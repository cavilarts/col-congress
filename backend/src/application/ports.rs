use std::future::Future;

use chrono::{DateTime, Utc};

use super::errors::{RepositoryError, SourceError};
use crate::domain::representative::Representative;
use crate::domain::senator::{Senator, SourceKind, Term};

pub trait SenatorSource: Send + Sync {
    fn kind(&self) -> SourceKind;
    fn fetch(&self, term: &Term) -> impl Future<Output = Result<Vec<Senator>, SourceError>> + Send;
}

pub trait SenatorRepository: Send + Sync {
    type Lock: Send;

    fn has_term(&self, term: &Term) -> impl Future<Output = Result<bool, RepositoryError>> + Send;
    fn last_updated(
        &self,
        term: &Term,
    ) -> impl Future<Output = Result<Option<DateTime<Utc>>, RepositoryError>> + Send;
    fn upsert_all(
        &self,
        senators: &[Senator],
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<usize, RepositoryError>> + Send;
    fn list(
        &self,
        term: Option<&Term>,
    ) -> impl Future<Output = Result<Vec<Senator>, RepositoryError>> + Send;
    fn try_sync_lock(
        &self,
    ) -> impl Future<Output = Result<Option<Self::Lock>, RepositoryError>> + Send;
}

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

pub trait RepresentativeSource: Send + Sync {
    fn kind(&self) -> SourceKind;
    fn fetch(
        &self,
        term: &Term,
    ) -> impl Future<Output = Result<Vec<Representative>, SourceError>> + Send;
}

pub trait RepresentativeRepository: Send + Sync {
    type Lock: Send;

    fn last_updated(
        &self,
        term: &Term,
    ) -> impl Future<Output = Result<Option<DateTime<Utc>>, RepositoryError>> + Send;
    fn upsert_all(
        &self,
        representatives: &[Representative],
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<usize, RepositoryError>> + Send;
    fn list(
        &self,
        term: Option<&Term>,
    ) -> impl Future<Output = Result<Vec<Representative>, RepositoryError>> + Send;
    fn try_sync_lock(
        &self,
    ) -> impl Future<Output = Result<Option<Self::Lock>, RepositoryError>> + Send;
}
