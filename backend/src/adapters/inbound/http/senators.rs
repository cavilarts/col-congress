use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::SecondsFormat;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::error;

use super::AppState;
use crate::application::errors::ListError;
use crate::application::ports::SenatorRepository;
use crate::domain::senator::Senator;

#[derive(Debug, Deserialize)]
pub struct SenatorsQuery {
    pub term: Option<String>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct SenatorResponse {
    pub id: String,
    pub term: String,
    pub full_name: String,
    pub gender: &'static str,
    pub birth_date: Option<String>,
    pub department_of_birth: Option<String>,
    pub city_of_birth: Option<String>,
    pub party: Option<String>,
    pub votes: Option<u64>,
    pub constitutional_commission: Option<String>,
    pub legal_commission: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub phone_extension: Option<String>,
    pub website: Option<String>,
    pub twitter: Option<String>,
    pub facebook: Option<String>,
    pub source: &'static str,
    pub updated_at: Option<String>,
}

impl From<Senator> for SenatorResponse {
    fn from(s: Senator) -> Self {
        Self {
            id: s.id.as_str().to_string(),
            term: s.term.as_str().to_string(),
            full_name: s.full_name,
            gender: s.gender.as_str(),
            birth_date: s.birth_date.map(|d| d.to_string()),
            department_of_birth: s.department_of_birth,
            city_of_birth: s.city_of_birth,
            party: s.party,
            votes: s.votes,
            constitutional_commission: s.constitutional_commission,
            legal_commission: s.legal_commission,
            email: s.email,
            phone: s.phone,
            phone_extension: s.phone_extension,
            website: s.website,
            twitter: s.twitter,
            facebook: s.facebook,
            source: s.source.as_str(),
            updated_at: s
                .updated_at
                .map(|at| at.to_rfc3339_opts(SecondsFormat::Secs, true)),
        }
    }
}

pub async fn list_senators<R: SenatorRepository + 'static, Q>(
    State(state): State<AppState<R, Q>>,
    Query(query): Query<SenatorsQuery>,
) -> Response {
    match state.senators.execute(query.term.as_deref()).await {
        Ok(senators) => {
            let body: Vec<SenatorResponse> =
                senators.into_iter().map(SenatorResponse::from).collect();
            Json(body).into_response()
        }
        Err(ListError::InvalidTerm) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid_term"})),
        )
            .into_response(),
        Err(ListError::Repository(e)) => {
            error!(error = %e, "listing senators failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "internal"})),
            )
                .into_response()
        }
    }
}
