use std::sync::Arc;

use axum::{Router, routing::get};

use crate::application::list_representatives::ListRepresentatives;
use crate::application::list_senators::ListSenators;
use crate::application::ports::{RepresentativeRepository, SenatorRepository};

pub mod representatives;
pub mod senators;

pub struct AppState<S, R> {
    pub senators: Arc<ListSenators<S>>,
    pub representatives: Arc<ListRepresentatives<R>>,
}

impl<S, R> Clone for AppState<S, R> {
    fn clone(&self) -> Self {
        Self {
            senators: Arc::clone(&self.senators),
            representatives: Arc::clone(&self.representatives),
        }
    }
}

pub fn router<S, R>(senators: ListSenators<S>, representatives: ListRepresentatives<R>) -> Router
where
    S: SenatorRepository + 'static,
    R: RepresentativeRepository + 'static,
{
    Router::new()
        .route("/health", get(health))
        .route("/api/senators", get(senators::list_senators::<S, R>))
        .route(
            "/api/representatives",
            get(representatives::list_representatives::<S, R>),
        )
        .with_state(AppState {
            senators: Arc::new(senators),
            representatives: Arc::new(representatives),
        })
}

async fn health() -> &'static str {
    "ok"
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use chrono::{DateTime, TimeZone, Utc};
    use http_body_util::BodyExt;
    use serde_json::json;
    use tower::ServiceExt;

    use super::senators::SenatorResponse;
    use super::*;
    use crate::application::errors::RepositoryError;
    use crate::domain::representative::Representative;
    use crate::domain::senator::{Senator, SourceKind, Term};

    struct FakeSenators {
        senators: Vec<Senator>,
        fail: bool,
    }

    impl SenatorRepository for FakeSenators {
        type Lock = ();

        async fn has_term(&self, _term: &Term) -> Result<bool, RepositoryError> {
            Ok(true)
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
            if self.fail {
                return Err(RepositoryError("secret connection string".into()));
            }
            Ok(self
                .senators
                .iter()
                .filter(|s| term.is_none_or(|t| &s.term == t))
                .cloned()
                .collect())
        }

        async fn try_sync_lock(&self) -> Result<Option<()>, RepositoryError> {
            Ok(Some(()))
        }
    }

    struct FakeReps {
        fail: bool,
    }

    impl RepresentativeRepository for FakeReps {
        type Lock = ();

        async fn last_updated(
            &self,
            _term: &Term,
        ) -> Result<Option<DateTime<Utc>>, RepositoryError> {
            Ok(None)
        }

        async fn upsert_all(
            &self,
            reps: &[Representative],
            _now: DateTime<Utc>,
        ) -> Result<usize, RepositoryError> {
            Ok(reps.len())
        }

        async fn list(&self, term: Option<&Term>) -> Result<Vec<Representative>, RepositoryError> {
            if self.fail {
                return Err(RepositoryError("secret reps".into()));
            }
            let mut r = Representative::new(
                Term::parse("2026-2030").unwrap(),
                "Ana Paola".into(),
                SourceKind::DatosGovCo,
            );
            r.constituency = Some("NARIÑO".into());
            r.updated_at = Some(Utc.with_ymd_and_hms(2026, 10, 11, 7, 0, 0).unwrap());
            Ok(vec![r]
                .into_iter()
                .filter(|r| term.is_none_or(|t| &r.term == t))
                .collect())
        }

        async fn try_sync_lock(&self) -> Result<Option<()>, RepositoryError> {
            Ok(Some(()))
        }
    }

    fn senator(term: &str, name: &str) -> Senator {
        let mut s = Senator::new(Term::parse(term).unwrap(), name.into(), SourceKind::Senado);
        s.party = Some("MIRA".into());
        s.birth_date = chrono::NaiveDate::from_ymd_opt(1971, 3, 25);
        s.updated_at = Some(Utc.with_ymd_and_hms(2026, 10, 11, 7, 0, 0).unwrap());
        s
    }

    fn app(fail: bool) -> Router {
        router(
            ListSenators::new(FakeSenators {
                senators: vec![
                    senator("2026-2030", "Ana Paola"),
                    senator("2018-2022", "Luis Pérez"),
                ],
                fail,
            }),
            ListRepresentatives::new(FakeReps { fail }),
        )
    }

    async fn get_json(app: Router, uri: &str) -> (StatusCode, serde_json::Value) {
        let response = app
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, value)
    }

    #[tokio::test]
    async fn senators_list_returns_expected_keys() {
        let (status, body) = get_json(app(false), "/api/senators").await;
        assert_eq!(status, StatusCode::OK);
        let items = body.as_array().unwrap();
        assert_eq!(items.len(), 2);
        for key in [
            "id",
            "term",
            "full_name",
            "gender",
            "birth_date",
            "department_of_birth",
            "city_of_birth",
            "party",
            "votes",
            "constitutional_commission",
            "legal_commission",
            "email",
            "phone",
            "phone_extension",
            "website",
            "twitter",
            "facebook",
            "source",
            "updated_at",
        ] {
            assert!(items[0].get(key).is_some(), "missing key {key}");
        }
        assert_eq!(items[0]["birth_date"], "1971-03-25");
        assert_eq!(items[0]["updated_at"], "2026-10-11T07:00:00Z");
        assert_eq!(items[0]["source"], "senado");
    }

    #[tokio::test]
    async fn senators_term_query_filters() {
        let (status, body) = get_json(app(false), "/api/senators?term=2026-2030").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn senators_invalid_term_is_400() {
        let (status, body) = get_json(app(false), "/api/senators?term=bad").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, json!({"error": "invalid_term"}));
    }

    #[tokio::test]
    async fn senators_repository_error_is_500_without_details() {
        let (status, body) = get_json(app(true), "/api/senators").await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body, json!({"error": "internal"}));
        assert!(!body.to_string().contains("secret"));
    }

    #[tokio::test]
    async fn representatives_list_includes_constituency() {
        let (status, body) = get_json(app(false), "/api/representatives?term=2026-2030").await;
        assert_eq!(status, StatusCode::OK);
        let items = body.as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["constituency"], "NARIÑO");
        assert_eq!(items[0]["source"], "datos_gov_co");
        assert_eq!(items[0]["updated_at"], "2026-10-11T07:00:00Z");
    }

    #[tokio::test]
    async fn representatives_invalid_term_is_400() {
        let (status, body) = get_json(app(false), "/api/representatives?term=2026").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, json!({"error": "invalid_term"}));
    }

    #[tokio::test]
    async fn representatives_repository_error_is_500_without_details() {
        let (status, body) = get_json(app(true), "/api/representatives").await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body, json!({"error": "internal"}));
        assert!(!body.to_string().contains("secret"));
    }

    #[tokio::test]
    async fn health_still_returns_ok() {
        let response = app(false)
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&bytes[..], b"ok");
    }

    #[test]
    fn senator_response_maps_missing_updated_at_to_null() {
        let mut s = senator("2026-2030", "Ana");
        s.updated_at = None;
        let response = SenatorResponse::from(s);
        assert_eq!(response.updated_at, None);
    }
}
