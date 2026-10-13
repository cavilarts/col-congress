pub mod adapters;
pub mod application;
pub mod config;
pub mod domain;

use std::process::ExitCode;

use sqlx::PgPool;
use tracing::{error, info};

use adapters::inbound::http;
use adapters::inbound::scheduler::{RepresentativesJob, SenatorsJob};
use adapters::outbound::clock::SystemClock;
use adapters::outbound::congreso_visible::{CongresoVisibleSource, LISTING_URL};
use adapters::outbound::datos_gov_co::{DatosGovCoSource, SENATORS_DATASET};
use adapters::outbound::postgres::{MIGRATOR, PgRepresentativeRepository, PgSenatorRepository};
use adapters::outbound::senado_web::{SENADO_SENATORS_URL, SenadoWebSource};
use application::list_representatives::ListRepresentatives;
use application::list_senators::ListSenators;
use application::sync_representatives::SyncRepresentatives;
use application::sync_senators::SyncSenators;
use config::Config;
use domain::senator::Term;

const DATOS_GOV_CO_BASE_URL: &str = "https://www.datos.gov.co";
const REPRESENTATIVES_DATASET: &str = "48i3-vuny";
const HISTORICAL_TERM: &str = "2018-2022";
const CURRENT_TERM: &str = "2026-2030";

#[tokio::main]
async fn main() -> ExitCode {
    let config = match Config::from_env() {
        Ok(config) => config,
        Err(e) => {
            eprintln!("configuration error: {e}");
            return ExitCode::FAILURE;
        }
    };

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(&config.log_filter))
        .init();

    let pool = match PgPool::connect(&config.database_url).await {
        Ok(pool) => pool,
        Err(e) => {
            error!(error = %e, "failed to connect to the database");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = MIGRATOR.run(&pool).await {
        error!(error = %e, "failed to run migrations");
        return ExitCode::FAILURE;
    }
    info!("migrations applied");

    let (Some(historical), Some(current)) =
        (Term::parse(HISTORICAL_TERM), Term::parse(CURRENT_TERM))
    else {
        error!("built-in terms are invalid");
        return ExitCode::FAILURE;
    };
    let (senators_dataset, senado, representatives_dataset, congreso_visible) = match (
        DatosGovCoSource::new(
            DATOS_GOV_CO_BASE_URL,
            SENATORS_DATASET,
            config.app_token.clone(),
        ),
        SenadoWebSource::new(SENADO_SENATORS_URL),
        DatosGovCoSource::new(
            DATOS_GOV_CO_BASE_URL,
            REPRESENTATIVES_DATASET,
            config.app_token.clone(),
        ),
        CongresoVisibleSource::new(LISTING_URL),
    ) {
        (Ok(a), Ok(b), Ok(c), Ok(d)) => (a, b, c, d),
        _ => {
            error!("failed to build HTTP clients");
            return ExitCode::FAILURE;
        }
    };

    let senators_job = SenatorsJob {
        sync: SyncSenators::new(PgSenatorRepository::new(pool.clone()), SystemClock),
        historical,
        current: current.clone(),
        dataset: senators_dataset,
        primary: senado,
        fallback: None::<SenadoWebSource>,
    };
    let representatives_job = RepresentativesJob {
        sync: SyncRepresentatives::new(PgRepresentativeRepository::new(pool.clone()), SystemClock),
        current,
        primary: representatives_dataset,
        fallback: Some(congreso_visible),
    };

    let senators_task = tokio::spawn(async move {
        senators_job.run_startup().await;
        senators_job.run_daily().await;
    });
    let representatives_task = tokio::spawn(async move {
        representatives_job.run_startup().await;
        representatives_job.run_daily().await;
    });
    for (job, task) in [
        ("senators", senators_task),
        ("representatives", representatives_task),
    ] {
        tokio::spawn(async move {
            match task.await {
                Ok(()) => error!(job, "scheduler task ended"),
                Err(e) => error!(job, error = %e, "scheduler task failed"),
            }
        });
    }

    let app = http::router(
        ListSenators::new(PgSenatorRepository::new(pool.clone())),
        ListRepresentatives::new(PgRepresentativeRepository::new(pool)),
    );
    let listener = match tokio::net::TcpListener::bind(config.bind_address).await {
        Ok(listener) => listener,
        Err(e) => {
            error!(error = %e, "failed to bind address");
            return ExitCode::FAILURE;
        }
    };
    info!(address = %config.bind_address, "serving");
    if let Err(e) = axum::serve(listener, app).await {
        error!(error = %e, "server error");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
