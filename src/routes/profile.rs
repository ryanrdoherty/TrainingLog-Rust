use crate::{
    error::AppError,
    models::{profile::UpdateProfileRequest, user::User},
};
use axum::{extract::State, Extension, Json};
use serde_json::Value;
use sqlx::PgPool;

pub async fn get_me(
    State((pool, _)): State<(PgPool, crate::config::Config)>,
    Extension(user): Extension<User>,
) -> Result<Json<serde_json::Value>, AppError> {
    #[derive(sqlx::FromRow)]
    struct ProfileRow {
        display_name: Option<String>,
        preferred_units: String,
        phone_number: Option<String>,
        phone_verified: bool,
        preferences: Value,
    }

    let profile: Option<ProfileRow> = sqlx::query_as(
        "SELECT display_name, preferred_units, phone_number, phone_verified, preferences
         FROM profiles WHERE user_id = $1",
    )
    .bind(user.id)
    .fetch_optional(&pool)
    .await?;

    Ok(Json(serde_json::json!({
        "id": user.id,
        "email": user.email,
        "created_at": user.created_at,
        "profile": profile.map(|p| serde_json::json!({
            "display_name": p.display_name,
            "preferred_units": p.preferred_units,
            "phone_number": p.phone_number,
            "phone_verified": p.phone_verified,
            "preferences": p.preferences,
        }))
    })))
}

pub async fn update_profile(
    State((pool, _)): State<(PgPool, crate::config::Config)>,
    Extension(user): Extension<User>,
    Json(body): Json<UpdateProfileRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if let Some(ref units) = body.preferred_units {
        if units != "metric" && units != "imperial" {
            return Err(AppError::BadRequest(
                "preferred_units must be 'metric' or 'imperial'".into(),
            ));
        }
    }

    sqlx::query(
        r#"
        UPDATE profiles SET
            display_name    = COALESCE($1, display_name),
            preferred_units = COALESCE($2, preferred_units),
            phone_number    = COALESCE($3, phone_number),
            updated_at      = now()
        WHERE user_id = $4
        "#,
    )
    .bind(body.display_name)
    .bind(body.preferred_units)
    .bind(body.phone_number)
    .bind(user.id)
    .execute(&pool)
    .await?;

    Ok(Json(serde_json::json!({ "message": "Profile updated" })))
}

pub async fn update_preferences(
    State((pool, _)): State<(PgPool, crate::config::Config)>,
    Extension(user): Extension<User>,
    Json(body): Json<Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !body.is_object() {
        return Err(AppError::BadRequest(
            "Preferences must be a JSON object".into(),
        ));
    }

    sqlx::query(
        "UPDATE profiles SET preferences = preferences || $1, updated_at = now() WHERE user_id = $2",
    )
    .bind(body)
    .bind(user.id)
    .execute(&pool)
    .await?;

    Ok(Json(serde_json::json!({ "message": "Preferences updated" })))
}
