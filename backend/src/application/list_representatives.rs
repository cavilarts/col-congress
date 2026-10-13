use super::errors::ListError;
use super::ports::RepresentativeRepository;
use crate::domain::representative::Representative;
use crate::domain::senator::Term;

pub struct ListRepresentatives<R> {
    repo: R,
}

impl<R: RepresentativeRepository> ListRepresentatives<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    pub async fn execute(&self, term: Option<&str>) -> Result<Vec<Representative>, ListError> {
        let term = match term {
            None => None,
            Some(raw) => Some(Term::parse(raw).ok_or(ListError::InvalidTerm)?),
        };
        Ok(self.repo.list(term.as_ref()).await?)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use chrono::{DateTime, Utc};

    use super::*;
    use crate::application::errors::{ListError, RepositoryError};
    use crate::domain::senator::SourceKind;

    struct Repo {
        fail: bool,
        seen: Mutex<Option<Option<String>>>,
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
            Ok(representatives.len())
        }

        async fn list(&self, term: Option<&Term>) -> Result<Vec<Representative>, RepositoryError> {
            *self.seen.lock().unwrap() = Some(term.map(|t| t.as_str().to_string()));
            if self.fail {
                return Err(RepositoryError("down".into()));
            }
            Ok(vec![Representative::new(
                Term::parse("2026-2030").unwrap(),
                "Ana".into(),
                SourceKind::DatosGovCo,
            )])
        }

        async fn try_sync_lock(&self) -> Result<Option<()>, RepositoryError> {
            Ok(Some(()))
        }
    }

    fn list(fail: bool) -> ListRepresentatives<Repo> {
        ListRepresentatives::new(Repo {
            fail,
            seen: Mutex::new(None),
        })
    }

    #[tokio::test]
    async fn valid_term_is_passed_through() {
        let l = list(false);
        assert_eq!(l.execute(Some("2026-2030")).await.unwrap().len(), 1);
        assert_eq!(
            *l.repo.seen.lock().unwrap(),
            Some(Some("2026-2030".to_string()))
        );
    }

    #[tokio::test]
    async fn no_term_lists_all() {
        let l = list(false);
        l.execute(None).await.unwrap();
        assert_eq!(*l.repo.seen.lock().unwrap(), Some(None));
    }

    #[tokio::test]
    async fn invalid_term_is_rejected_before_repository() {
        let l = list(false);
        assert_eq!(l.execute(Some("2026")).await, Err(ListError::InvalidTerm));
        assert!(l.repo.seen.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn repository_error_is_propagated() {
        assert_eq!(
            list(true).execute(None).await,
            Err(ListError::Repository(RepositoryError("down".into())))
        );
    }
}
