use crate::{auth, config::Config, metrics};
use axum::{
    middleware,
    routing::{delete, get, post, put},
    Router,
};
use axum_prometheus::{metrics_exporter_prometheus::PrometheusHandle, PrometheusMetricLayer};
use sqlx::PgPool;

pub mod activities;
pub mod profile;

pub fn app_router(
    pool: PgPool,
    config: Config,
    metrics_handle: PrometheusHandle,
    metric_layer: PrometheusMetricLayer<'static>,
) -> Router {
    let state = (pool.clone(), config.clone());

    // Public auth routes
    let auth_routes = Router::new()
        .route("/register", post(auth::local::register))
        .route("/login", post(auth::local::login))
        .route("/verify-email", post(auth::local::verify_email))
        .route("/forgot-password", post(auth::local::forgot_password))
        .route("/reset-password", post(auth::local::reset_password))
        .route("/otp/request", post(auth::otp::request_otp))
        .route("/otp/verify", post(auth::otp::verify_otp))
        .route("/login/:provider", get(auth::oauth::login))
        .route("/callback/:provider", get(auth::oauth::callback))
        .with_state(state.clone());

    // Protected routes (require valid JWT)
    let protected_routes = Router::new()
        .route("/me", get(profile::get_me))
        .route("/me/profile", put(profile::update_profile))
        .route("/me/preferences", put(profile::update_preferences))
        .route("/activities", get(activities::list_activities))
        .route("/activities", post(activities::create_activity))
        .route("/activities/:id", get(activities::get_activity))
        .route("/activities/:id", put(activities::update_activity))
        .route("/activities/:id", delete(activities::delete_activity))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::require_auth,
        ))
        .with_state(state.clone());

    // /metrics endpoint — state is just the PrometheusHandle
    let metrics_route = Router::new()
        .route("/metrics", get(metrics::metrics_handler))
        .with_state(metrics_handle);

    Router::new()
        .nest("/auth", auth_routes)
        .merge(protected_routes)
        .merge(metrics_route)
        .layer(metric_layer) // HTTP request metrics on all routes
}
