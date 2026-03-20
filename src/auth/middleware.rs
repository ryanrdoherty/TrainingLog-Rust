use crate::{auth::jwt, config::Config, error::AppError, models::user::User};
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use sqlx::PgPool;
use uuid::Uuid;

/// Axum middleware that validates the Bearer JWT and injects the authenticated user.
pub async fn require_auth(
    State((pool, config)): State<(PgPool, Config)>,
    mut req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let token = extract_bearer(&req)?;
    let claims = jwt::verify_token(token, &config)?;

    let user_id: Uuid = claims
        .sub
        .parse()
        .map_err(|_| AppError::Unauthorized)?;

    let user = sqlx::query_as::<_, User>(
        "SELECT id, email, created_at, updated_at, token_version FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(&pool)
    .await?
    .ok_or(AppError::Unauthorized)?;

    // Reject tokens issued before a password reset
    if user.token_version != claims.ver {
        return Err(AppError::Unauthorized);
    }

    req.extensions_mut().insert(user);
    Ok(next.run(req).await)
}

fn extract_bearer(req: &Request) -> Result<&str, AppError> {
    req.headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(AppError::Unauthorized)
}
