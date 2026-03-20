use crate::{auth::jwt, config::Config, error::AppError};
use metrics::counter;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{extract::State, Json};
use chrono::{Duration, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

// ── Request / Response types ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct VerifyEmailRequest {
    pub token: String,
}

#[derive(Debug, Deserialize)]
pub struct ForgotPasswordRequest {
    pub email: String,
}

#[derive(Debug, Deserialize)]
pub struct ResetPasswordRequest {
    pub token: String,
    pub new_password: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub token: String,
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn hash_password(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Password hash error: {e}")))
}

fn verify_password(hash: &str, password: &str) -> Result<bool, AppError> {
    let parsed = PasswordHash::new(hash)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Password hash parse error: {e}")))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn generate_token() -> String {
    let bytes: [u8; 32] = rand::thread_rng().r#gen();
    hex::encode(bytes)
}

fn validate_password(password: &str) -> Result<(), AppError> {
    if password.len() < 8 {
        return Err(AppError::BadRequest(
            "Password must be at least 8 characters".into(),
        ));
    }
    Ok(())
}

// ── Handlers ─────────────────────────────────────────────────────────────────

pub async fn register(
    State((pool, _config)): State<(PgPool, Config)>,
    Json(body): Json<RegisterRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    validate_password(&body.password)?;

    let email = body.email.to_lowercase();

    // Check for existing user
    let existing: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM users WHERE email = $1")
            .bind(&email)
            .fetch_optional(&pool)
            .await?;

    if existing.is_some() {
        return Err(AppError::Conflict("Email already registered".into()));
    }

    let password_hash = hash_password(&body.password)?;
    let user_id = Uuid::new_v4();

    let mut tx = pool.begin().await?;

    sqlx::query("INSERT INTO users (id, email) VALUES ($1, $2)")
        .bind(user_id)
        .bind(&email)
        .execute(&mut *tx)
        .await?;

    sqlx::query("INSERT INTO profiles (user_id) VALUES ($1)")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

    let verify_token = generate_token();
    let verify_hash = hash_token(&verify_token);
    let verify_expires = Utc::now() + Duration::hours(24);

    sqlx::query(
        "INSERT INTO local_credentials (user_id, password_hash, verify_token, verify_expires)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(user_id)
    .bind(&password_hash)
    .bind(&verify_hash)
    .bind(verify_expires)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    counter!("sports_log_users_registered_total").increment(1);
    // TODO: send verification email with verify_token
    tracing::info!(user_id = %user_id, "User registered; verification token: {verify_token}");

    Ok(Json(serde_json::json!({
        "message": "Registration successful. Please check your email to verify your account."
    })))
}

pub async fn login(
    State((pool, config)): State<(PgPool, Config)>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    let email = body.email.to_lowercase();

    #[derive(sqlx::FromRow)]
    struct LoginRow {
        id: Uuid,
        token_version: i32,
        password_hash: String,
        email_verified: bool,
    }

    // Fetch user + credentials in one join; use identical error for not-found and wrong password
    let row: Option<LoginRow> = sqlx::query_as(
        r#"
        SELECT u.id, u.token_version, lc.password_hash, lc.email_verified
        FROM users u
        JOIN local_credentials lc ON lc.user_id = u.id
        WHERE u.email = $1
        "#,
    )
    .bind(&email)
    .fetch_optional(&pool)
    .await?;

    let row = row.ok_or(AppError::Unauthorized)?;

    if !verify_password(&row.password_hash, &body.password)? {
        counter!("sports_log_logins_total", "method" => "local", "status" => "failure").increment(1);
        return Err(AppError::Unauthorized);
    }

    if !row.email_verified {
        return Err(AppError::Forbidden(
            "Please verify your email before logging in".into(),
        ));
    }

    let token = jwt::issue_token(row.id, &email, row.token_version, &config)?;
    counter!("sports_log_logins_total", "method" => "local", "status" => "success").increment(1);
    Ok(Json(AuthResponse { token }))
}

pub async fn verify_email(
    State((pool, config)): State<(PgPool, Config)>,
    Json(body): Json<VerifyEmailRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    let token_hash = hash_token(&body.token);

    #[derive(sqlx::FromRow)]
    struct VerifyRow {
        id: Uuid,
        email: String,
        token_version: i32,
    }

    let row: Option<VerifyRow> = sqlx::query_as(
        r#"
        UPDATE local_credentials lc
        SET email_verified = true, verify_token = NULL, verify_expires = NULL,
            updated_at = now()
        FROM users u
        WHERE lc.user_id = u.id
          AND lc.verify_token = $1
          AND lc.verify_expires > now()
          AND lc.email_verified = false
        RETURNING u.id, u.email, u.token_version
        "#,
    )
    .bind(&token_hash)
    .fetch_optional(&pool)
    .await?;

    let row =
        row.ok_or_else(|| AppError::BadRequest("Invalid or expired verification token".into()))?;

    let token = jwt::issue_token(row.id, &row.email, row.token_version, &config)?;
    Ok(Json(AuthResponse { token }))
}

pub async fn forgot_password(
    State((pool, _config)): State<(PgPool, Config)>,
    Json(body): Json<ForgotPasswordRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let email = body.email.to_lowercase();

    #[derive(sqlx::FromRow)]
    struct UserRow {
        id: Uuid,
    }

    // Always return 200 to prevent email enumeration
    let user: Option<UserRow> = sqlx::query_as(
        "SELECT u.id FROM users u JOIN local_credentials lc ON lc.user_id = u.id WHERE u.email = $1",
    )
    .bind(&email)
    .fetch_optional(&pool)
    .await?;

    if let Some(user) = user {
        let reset_token = generate_token();
        let reset_hash = hash_token(&reset_token);
        let reset_expires = Utc::now() + Duration::hours(1);

        sqlx::query(
            "UPDATE local_credentials SET reset_token = $1, reset_expires = $2, updated_at = now()
             WHERE user_id = $3",
        )
        .bind(&reset_hash)
        .bind(reset_expires)
        .bind(user.id)
        .execute(&pool)
        .await?;

        // TODO: send password reset email with reset_token
        tracing::info!(user_id = %user.id, "Password reset token generated: {reset_token}");
    }

    Ok(Json(serde_json::json!({
        "message": "If that email is registered, you will receive a password reset link."
    })))
}

pub async fn reset_password(
    State((pool, _config)): State<(PgPool, Config)>,
    Json(body): Json<ResetPasswordRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    validate_password(&body.new_password)?;

    let token_hash = hash_token(&body.token);
    let new_hash = hash_password(&body.new_password)?;

    // Atomically validate token and update password
    let updated = sqlx::query(
        r#"
        UPDATE local_credentials lc
        SET password_hash = $1, reset_token = NULL, reset_expires = NULL, updated_at = now()
        FROM users u
        WHERE lc.user_id = u.id
          AND lc.reset_token = $2
          AND lc.reset_expires > now()
        "#,
    )
    .bind(&new_hash)
    .bind(&token_hash)
    .execute(&pool)
    .await?;

    if updated.rows_affected() == 0 {
        return Err(AppError::BadRequest("Invalid or expired reset token".into()));
    }

    // Increment token_version to invalidate all existing JWTs
    sqlx::query(
        r#"
        UPDATE users u
        SET token_version = token_version + 1, updated_at = now()
        FROM local_credentials lc
        WHERE u.id = lc.user_id AND lc.reset_token IS NULL AND lc.password_hash = $1
        "#,
    )
    .bind(&new_hash)
    .execute(&pool)
    .await?;

    Ok(Json(serde_json::json!({
        "message": "Password reset successful. Please log in with your new password."
    })))
}
