use crate::config::Config;
use crate::error::AppError;
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,       // user id
    pub email: String,
    pub ver: i32,          // token_version — incremented on password reset
    pub exp: i64,          // expiry timestamp
    pub iat: i64,          // issued at
}

pub fn issue_token(
    user_id: Uuid,
    email: &str,
    token_version: i32,
    config: &Config,
) -> Result<String, AppError> {
    let now = Utc::now();
    let expiry = now + Duration::hours(config.jwt_expiry_hours);

    let claims = Claims {
        sub: user_id.to_string(),
        email: email.to_owned(),
        ver: token_version,
        exp: expiry.timestamp(),
        iat: now.timestamp(),
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(config.jwt_secret.as_bytes()),
    )
    .map_err(|e| AppError::Internal(anyhow::anyhow!("JWT encode error: {e}")))
}

pub fn verify_token(token: &str, config: &Config) -> Result<Claims, AppError> {
    let mut validation = Validation::default();
    validation.leeway = 10; // 10-second clock skew tolerance

    decode::<Claims>(
        token,
        &DecodingKey::from_secret(config.jwt_secret.as_bytes()),
        &validation,
    )
    .map(|data| data.claims)
    .map_err(|_| AppError::Unauthorized)
}
