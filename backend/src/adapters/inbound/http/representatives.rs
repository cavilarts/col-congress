use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::SecondsFormat;
use serde::Serialize;
use serde_json::json;
use tracing::error;

use super::AppState;
use super::senators::SenatorsQuery;
use crate::application::errors::ListError;
use crate::application::ports::RepresentativeRepository;
use crate::domain::representative::Representative;

#[derive(Debug, Serialize, PartialEq)]
pub struct RepresentativeResponse {
    pub id: String,
    pub term: String,
    pub full_name: String,
    pub gender: &'static str,
    pub birth_date: Option<String>,
    pub department_of_birth: Option<String>,
    pub city_of_birth: Option<String>,
    pub party: Option<String>,
    pub constituency: Option<String>,
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

impl From<Representative> for RepresentativeResponse {
    fn from(r: Representative) -> Self {
        Self {
            id: r.id.as_str().to_string(),
            term: r.term.as_str().to_string(),
            full_name: r.full_name,
            gender: r.gender.as_str(),
            birth_date: r.birth_date.map(|d| d.to_string()),
            department_of_birth: r.department_of_birth,
            city_of_birth: r.city_of_birth,
            party: r.party,
            constituency: r.constituency,
            votes: r.votes,
            constitutional_commission: r.constitutional_commission,
            legal_commission: r.legal_commission,
            email: r.email,
            phone: r.phone,
            phone_extension: r.phone_extension,
            website: r.website,
            twitter: r.twitter,
            facebook: r.facebook,
            source: r.source.as_str(),
            updated_at: r
                .updated_at
                .map(|at| at.to_rfc3339_opts(SecondsFormat::Secs, true)),
        }
    }
}

pub async fn list_representatives<S, R: RepresentativeRepository + 'static>(
    State(state): State<AppState<S, R>>,
    Query(query): Query<SenatorsQuery>,
) -> Response {
    match state.representatives.execute(query.term.as_deref()).await {
        Ok(representatives) => {
            let body: Vec<RepresentativeResponse> = representatives
                .into_iter()
                .map(RepresentativeResponse::from)
                .collect();
            Json(body).into_response()
        }
        Err(ListError::InvalidTerm) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid_term"})),
        )
            .into_response(),
        Err(ListError::Repository(e)) => {
            error!(error = %e, "listing representatives failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "internal"})),
            )
                .into_response()
        }
    }
}
