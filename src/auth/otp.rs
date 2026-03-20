use crate::{auth::jwt, config::Config, error::AppError};
use metrics::counter;
use axum::{extract::State, Json};
use chrono::{Duration, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

const OTP_TTL_MINUTES: i64 = 10;
const OTP_MAX_ATTEMPTS: i32 = 5;
const OTP_RATE_LIMIT_WINDOW_MINUTES: i64 = 15;
const OTP_RATE_LIMIT_MAX: i64 = 3;

#[derive(Debug, Deserialize)]
pub struct OtpRequestBody {
    /// Email address or phone number
    pub identifier: String,
    /// "email" or "sms"
    pub channel: String,
}

#[derive(Debug, Deserialize)]
pub struct OtpVerifyBody {
    pub identifier: String,
    pub code: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub token: String,
}

fn hash_code(code: &str) -> String {
    hex::encode(Sha256::digest(code.as_bytes()))
}

fn generate_otp() -> String {
    let code: u32 = rand::thread_rng().gen_range(0..1_000_000);
    format!("{code:06}")
}

pub async fn request_otp(
    State((pool, _config)): State<(PgPool, Config)>,
    Json(body): Json<OtpRequestBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    if body.channel != "email" && body.channel != "sms" {
        return Err(AppError::BadRequest("channel must be 'email' or 'sms'".into()));
    }

    // Normalize identifier
    let identifier = body.identifier.trim().to_lowercase();

    #[derive(sqlx::FromRow)]
    struct UserRow {
        id: Uuid,
    }

    // Look up user by email or phone
    let user: Option<UserRow> = match body.channel.as_str() {
        "email" => sqlx::query_as("SELECT u.id FROM users u WHERE u.email = $1")
            .bind(&identifier)
            .fetch_optional(&pool)
            .await?,
        "sms" => sqlx::query_as(
            "SELECT u.id FROM users u JOIN profiles p ON p.user_id = u.id
             WHERE p.phone_number = $1 AND p.phone_verified = true",
        )
        .bind(&identifier)
        .fetch_optional(&pool)
        .await?,
        _ => unreachable!(),
    };

    // Always return 200 (prevents enumeration)
    if let Some(user) = user {
        // Rate limit: max OTP_RATE_LIMIT_MAX requests per window
        let window_start = Utc::now() - Duration::minutes(OTP_RATE_LIMIT_WINDOW_MINUTES);
        let recent_count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM otp_challenges WHERE user_id = $1 AND created_at > $2",
        )
        .bind(user.id)
        .bind(window_start)
        .fetch_one(&pool)
        .await?;

        if recent_count.0 >= OTP_RATE_LIMIT_MAX {
            return Err(AppError::TooManyRequests);
        }

        // Invalidate previous unused OTPs
        sqlx::query("UPDATE otp_challenges SET used = true WHERE user_id = $1 AND used = false")
            .bind(user.id)
            .execute(&pool)
            .await?;

        let code = generate_otp();
        let code_hash = hash_code(&code);
        let expires_at = Utc::now() + Duration::minutes(OTP_TTL_MINUTES);

        sqlx::query(
            "INSERT INTO otp_challenges (user_id, channel, destination, code_hash, expires_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(user.id)
        .bind(&body.channel)
        .bind(&identifier)
        .bind(&code_hash)
        .bind(expires_at)
        .execute(&pool)
        .await?;

        // TODO: send via email (lettre) or SMS (Twilio)
        tracing::info!(
            user_id = %user.id,
            channel = body.channel,
            "OTP code generated: {code}"
        );
    }

    Ok(Json(serde_json::json!({
        "message": "If that account exists, a code has been sent."
    })))
}

pub async fn verify_otp(
    State((pool, config)): State<(PgPool, Config)>,
    Json(body): Json<OtpVerifyBody>,
) -> Result<Json<AuthResponse>, AppError> {
    let identifier = body.identifier.trim().to_lowercase();

    #[derive(sqlx::FromRow)]
    struct UserRow {
        id: Uuid,
        email: String,
        token_version: i32,
    }

    // Look up user
    let user: Option<UserRow> = sqlx::query_as(
        r#"
        SELECT u.id, u.email, u.token_version
        FROM users u
        LEFT JOIN profiles p ON p.user_id = u.id
        WHERE u.email = $1 OR p.phone_number = $1
        "#,
    )
    .bind(&identifier)
    .fetch_optional(&pool)
    .await?;

    let user = user.ok_or(AppError::Unauthorized)?;

    #[derive(sqlx::FromRow)]
    struct ChallengeRow {
        id: Uuid,
        code_hash: String,
        attempts: i32,
    }

    // Find the most recent valid challenge
    let challenge: Option<ChallengeRow> = sqlx::query_as(
        r#"
        SELECT id, code_hash, attempts
        FROM otp_challenges
        WHERE user_id = $1
          AND destination = $2
          AND used = false
          AND expires_at > now()
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .bind(user.id)
    .bind(&identifier)
    .fetch_optional(&pool)
    .await?;

    let challenge = challenge.ok_or(AppError::Unauthorized)?;

    // Increment attempt count first
    let new_attempts = challenge.attempts + 1;
    sqlx::query("UPDATE otp_challenges SET attempts = $1 WHERE id = $2")
        .bind(new_attempts)
        .bind(challenge.id)
        .execute(&pool)
        .await?;

    // Invalidate if max attempts exceeded
    if new_attempts > OTP_MAX_ATTEMPTS {
        sqlx::query("UPDATE otp_challenges SET used = true WHERE id = $1")
            .bind(challenge.id)
            .execute(&pool)
            .await?;
        return Err(AppError::Unauthorized);
    }

    // Verify code
    if hash_code(&body.code) != challenge.code_hash {
        return Err(AppError::Unauthorized);
    }

    // Mark as used
    sqlx::query("UPDATE otp_challenges SET used = true WHERE id = $1")
        .bind(challenge.id)
        .execute(&pool)
        .await?;

    let token = jwt::issue_token(user.id, &user.email, user.token_version, &config)?;
    counter!("sports_log_logins_total", "method" => "otp", "status" => "success").increment(1);
    Ok(Json(AuthResponse { token }))
}
