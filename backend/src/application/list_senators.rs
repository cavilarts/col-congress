use super::errors::ListError;
use super::ports::SenatorRepository;
use crate::domain::senator::{Senator, Term};

pub struct ListSenators<R> {
    repo: R,
}

impl<R: SenatorRepository> ListSenators<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    pub async fn execute(&self, term: Option<&str>) -> Result<Vec<Senator>, ListError> {
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
    use crate::application::errors::RepositoryError;
    use crate::domain::senator::SourceKind;

    struct FakeRepo {
        senators: Vec<Senator>,
        seen_term: Mutex<Option<Option<String>>>,
        fail: bool,
    }

    impl SenatorRepository for FakeRepo {
        type Lock = ();

        async fn has_term(&self, _term: &Term) -> Result<bool, RepositoryError> {
            Ok(false)
        }

        async fn last_updated(
            &self,
            _term: &Term,
        ) -> Result<Option<DateTime<Utc>>, RepositoryError> {
            Ok(None)
        }

        async fn upsert_all(
            &self,
            senators: &[Senator],
            _now: DateTime<Utc>,
        ) -> Result<usize, RepositoryError> {
            Ok(senators.len())
        }

        async fn list(&self, term: Option<&Term>) -> Result<Vec<Senator>, RepositoryError> {
            *self.seen_term.lock().unwrap() = Some(term.map(|t| t.as_str().to_string()));
            if self.fail {
                return Err(RepositoryError("down".into()));
            }
            Ok(self.senators.clone())
        }

        async fn try_sync_lock(&self) -> Result<Option<()>, RepositoryError> {
            Ok(Some(()))
        }
    }

    fn repo(fail: bool) -> FakeRepo {
        let term = Term::parse("2026-2030").unwrap();
        FakeRepo {
            senators: vec![Senator::new(term, "Ana Paola".into(), SourceKind::Senado)],
            seen_term: Mutex::new(None),
            fail,
        }
    }

    #[tokio::test]
    async fn valid_term_is_passed_through() {
        let list = ListSenators::new(repo(false));
        let result = list.execute(Some("2026-2030")).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            *list.repo.seen_term.lock().unwrap(),
            Some(Some("2026-2030".to_string()))
        );
    }

    #[tokio::test]
    async fn no_term_lists_everything() {
        let list = ListSenators::new(repo(false));
        list.execute(None).await.unwrap();
        assert_eq!(*list.repo.seen_term.lock().unwrap(), Some(None));
    }

    #[tokio::test]
    async fn invalid_term_is_rejected_before_repository() {
        let list = ListSenators::new(repo(false));
        let result = list.execute(Some("2026")).await;
        assert_eq!(result, Err(ListError::InvalidTerm));
        assert!(list.repo.seen_term.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn repository_error_is_propagated() {
        let list = ListSenators::new(repo(true));
        let result = list.execute(None).await;
        assert_eq!(
            result,
            Err(ListError::Repository(RepositoryError("down".into())))
        );
    }
}
