//! OAuth2 / OpenID Connect handlers for Google and Facebook.
//!
//! Flow:
//!   GET /auth/login/:provider  → redirect to provider
//!   GET /auth/callback/:provider → exchange code, upsert user, issue JWT

use crate::{auth::jwt, config::Config, error::AppError};
use metrics::counter;
use axum::{
    extract::{Path, Query, State},
    response::Redirect,
};
use oauth2::{
    basic::BasicClient, AuthUrl, ClientId, ClientSecret, CsrfToken, RedirectUrl, Scope, TokenUrl,
};
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub token: String,
}

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    pub code: String,
    pub state: String,
}

/// Builds the OAuth2 client for a given provider.
fn oauth_client(provider: &str, config: &Config) -> Result<BasicClient, AppError> {
    let (client_id, client_secret, auth_url, token_url) = match provider {
        "google" => (
            config.google_client_id.clone(),
            config.google_client_secret.clone(),
            "https://accounts.google.com/o/oauth2/v2/auth",
            "https://oauth2.googleapis.com/token",
        ),
        "facebook" => (
            config.facebook_client_id.clone(),
            config.facebook_client_secret.clone(),
            "https://www.facebook.com/v18.0/dialog/oauth",
            "https://graph.facebook.com/v18.0/oauth/access_token",
        ),
        _ => return Err(AppError::NotFound),
    };

    let redirect_url = format!("{}/auth/callback/{}", config.app_base_url, provider);

    Ok(BasicClient::new(
        ClientId::new(client_id),
        Some(ClientSecret::new(client_secret)),
        AuthUrl::new(auth_url.into()).unwrap(),
        Some(TokenUrl::new(token_url.into()).unwrap()),
    )
    .set_redirect_uri(RedirectUrl::new(redirect_url).unwrap()))
}

/// GET /auth/login/:provider
pub async fn login(
    Path(provider): Path<String>,
    State((_pool, config)): State<(PgPool, Config)>,
) -> Result<Redirect, AppError> {
    let client = oauth_client(&provider, &config)?;

    let scopes: Vec<Scope> = match provider.as_str() {
        "google" => vec![
            Scope::new("openid".into()),
            Scope::new("email".into()),
            Scope::new("profile".into()),
        ],
        "facebook" => vec![
            Scope::new("email".into()),
            Scope::new("public_profile".into()),
        ],
        _ => return Err(AppError::NotFound),
    };

    let (auth_url, _csrf_token) = client
        .authorize_url(CsrfToken::new_random)
        .add_scopes(scopes)
        .url();

    // NOTE: In production, store csrf_token in a short-lived signed cookie and
    // validate it in the callback to prevent CSRF attacks.
    tracing::debug!(provider = %provider, "Redirecting to OAuth2 provider");

    Ok(Redirect::to(auth_url.as_str()))
}

/// GET /auth/callback/:provider
pub async fn callback(
    Path(provider): Path<String>,
    Query(params): Query<CallbackQuery>,
    State((pool, config)): State<(PgPool, Config)>,
) -> Result<axum::response::Redirect, AppError> {
    let http = HttpClient::new();

    // Exchange authorization code for access token using reqwest directly
    let token_url = match provider.as_str() {
        "google" => "https://oauth2.googleapis.com/token",
        "facebook" => "https://graph.facebook.com/v18.0/oauth/access_token",
        _ => return Err(AppError::NotFound),
    };

    let redirect_uri = format!("{}/auth/callback/{}", config.app_base_url, provider);
    let (client_id, client_secret) = match provider.as_str() {
        "google" => (
            config.google_client_id.as_str(),
            config.google_client_secret.as_str(),
        ),
        _ => (
            config.facebook_client_id.as_str(),
            config.facebook_client_secret.as_str(),
        ),
    };

    let token_resp = http
        .post(token_url)
        .form(&[
            ("code", params.code.as_str()),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("redirect_uri", redirect_uri.as_str()),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Token exchange request failed: {e}")))?;

    #[derive(Deserialize)]
    struct TokenData {
        access_token: String,
    }

    let token_data: TokenData = token_resp
        .json()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Token exchange parse failed: {e}")))?;

    let access_token = token_data.access_token;

    // Fetch user info from provider
    let (email, provider_uid) = match provider.as_str() {
        "google" => fetch_google_user(&http, &access_token).await?,
        "facebook" => fetch_facebook_user(&http, &access_token).await?,
        _ => return Err(AppError::NotFound),
    };

    // Upsert user and oauth_connection
    let user = upsert_oauth_user(&pool, &email, &provider, &provider_uid, &access_token).await?;

    let token = jwt::issue_token(user.id, &user.email, user.token_version, &config)?;
    counter!("sports_log_logins_total", "method" => provider.clone(), "status" => "success").increment(1);
    let redirect_url = format!("{}/auth/callback?token={}", config.app_base_url, token);
    Ok(axum::response::Redirect::to(&redirect_url))
}

#[derive(Deserialize)]
struct GoogleUserInfo {
    sub: String,
    email: String,
}

async fn fetch_google_user(
    http: &HttpClient,
    access_token: &str,
) -> Result<(String, String), AppError> {
    let info: GoogleUserInfo = http
        .get("https://openidconnect.googleapis.com/v1/userinfo")
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Google userinfo request failed: {e}")))?
        .json()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Google userinfo parse failed: {e}")))?;

    Ok((info.email, info.sub))
}

#[derive(Deserialize)]
struct FacebookUserInfo {
    id: String,
    email: String,
}

async fn fetch_facebook_user(
    http: &HttpClient,
    access_token: &str,
) -> Result<(String, String), AppError> {
    let info: FacebookUserInfo = http
        .get("https://graph.facebook.com/me")
        .query(&[("fields", "id,email"), ("access_token", access_token)])
        .send()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Facebook user request failed: {e}")))?
        .json()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Facebook user parse failed: {e}")))?;

    Ok((info.email, info.id))
}

struct OauthUser {
    id: Uuid,
    email: String,
    token_version: i32,
}

async fn upsert_oauth_user(
    pool: &PgPool,
    email: &str,
    provider: &str,
    provider_uid: &str,
    access_token: &str,
) -> Result<OauthUser, AppError> {
    #[derive(sqlx::FromRow)]
    struct UserRow {
        id: Uuid,
        email: String,
        token_version: i32,
    }

    let mut tx = pool.begin().await?;

    // Upsert user by email
    let user: UserRow = sqlx::query_as(
        r#"
        INSERT INTO users (id, email)
        VALUES (gen_random_uuid(), $1)
        ON CONFLICT (email) DO UPDATE SET updated_at = now()
        RETURNING id, email, token_version
        "#,
    )
    .bind(email)
    .fetch_one(&mut *tx)
    .await?;

    // Ensure profile row exists
    sqlx::query("INSERT INTO profiles (user_id) VALUES ($1) ON CONFLICT DO NOTHING")
        .bind(user.id)
        .execute(&mut *tx)
        .await?;

    // Upsert oauth connection
    sqlx::query(
        r#"
        INSERT INTO oauth_connections (user_id, provider, provider_uid, access_token)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (provider, provider_uid)
        DO UPDATE SET access_token = EXCLUDED.access_token, updated_at = now()
        "#,
    )
    .bind(user.id)
    .bind(provider)
    .bind(provider_uid)
    .bind(access_token)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(OauthUser {
        id: user.id,
        email: user.email,
        token_version: user.token_version,
    })
}
