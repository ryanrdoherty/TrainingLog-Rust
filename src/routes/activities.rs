use crate::{
    error::AppError,
    models::{
        activity::{ActivityQuery, CreateActivityRequest, UpdateActivityRequest},
        user::User,
    },
};
use metrics::counter;
use axum::{
    extract::{Path, Query, State},
    Extension, Json,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

type AppState = (PgPool, crate::config::Config);

#[derive(sqlx::FromRow)]
struct ActivityRow {
    id: Uuid,
    activity_type: String,
    started_at: DateTime<Utc>,
    duration_secs: i32,
    distance_meters: Option<f32>,
    calories: Option<i32>,
    notes: Option<String>,
    source: String,
    device_data: Option<Value>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

pub async fn list_activities(
    State((pool, _)): State<AppState>,
    Extension(user): Extension<User>,
    Query(q): Query<ActivityQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let limit = q.limit.unwrap_or(20).min(100);
    let offset = q.offset.unwrap_or(0);

    let activities: Vec<ActivityRow> = sqlx::query_as(
        r#"
        SELECT id, activity_type, started_at, duration_secs, distance_meters,
               calories, notes, source, device_data, created_at, updated_at
        FROM activities
        WHERE user_id = $1
          AND ($2::text IS NULL OR activity_type = $2)
          AND ($3::timestamptz IS NULL OR started_at >= $3)
          AND ($4::timestamptz IS NULL OR started_at <= $4)
        ORDER BY started_at DESC
        LIMIT $5 OFFSET $6
        "#,
    )
    .bind(user.id)
    .bind(q.activity_type)
    .bind(q.from)
    .bind(q.to)
    .bind(limit)
    .bind(offset)
    .fetch_all(&pool)
    .await?;

    let items: Vec<serde_json::Value> = activities
        .iter()
        .map(|a| {
            serde_json::json!({
                "id": a.id,
                "activity_type": a.activity_type,
                "started_at": a.started_at,
                "duration_secs": a.duration_secs,
                "distance_meters": a.distance_meters,
                "calories": a.calories,
                "notes": a.notes,
                "source": a.source,
                "created_at": a.created_at,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "activities": items,
        "limit": limit,
        "offset": offset,
    })))
}

pub async fn create_activity(
    State((pool, _)): State<AppState>,
    Extension(user): Extension<User>,
    Json(body): Json<CreateActivityRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if body.duration_secs < 0 {
        return Err(AppError::BadRequest(
            "duration_secs must be non-negative".into(),
        ));
    }

    let id: (Uuid,) = sqlx::query_as(
        r#"
        INSERT INTO activities
            (user_id, activity_type, started_at, duration_secs, distance_meters,
             calories, notes, source, device_data)
        VALUES ($1, $2, $3, $4, $5, $6, $7, 'manual', $8)
        RETURNING id
        "#,
    )
    .bind(user.id)
    .bind(body.activity_type)
    .bind(body.started_at)
    .bind(body.duration_secs)
    .bind(body.distance_meters)
    .bind(body.calories)
    .bind(body.notes)
    .bind(body.device_data)
    .fetch_one(&pool)
    .await?;

    counter!("sports_log_activities_created_total").increment(1);
    Ok(Json(serde_json::json!({ "id": id.0, "message": "Activity created" })))
}

pub async fn get_activity(
    State((pool, _)): State<AppState>,
    Extension(user): Extension<User>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let activity: Option<ActivityRow> = sqlx::query_as(
        r#"
        SELECT id, activity_type, started_at, duration_secs, distance_meters,
               calories, notes, source, device_data, created_at, updated_at
        FROM activities
        WHERE id = $1 AND user_id = $2
        "#,
    )
    .bind(id)
    .bind(user.id)
    .fetch_optional(&pool)
    .await?;

    let activity = activity.ok_or(AppError::NotFound)?;

    Ok(Json(serde_json::json!({
        "id": activity.id,
        "activity_type": activity.activity_type,
        "started_at": activity.started_at,
        "duration_secs": activity.duration_secs,
        "distance_meters": activity.distance_meters,
        "calories": activity.calories,
        "notes": activity.notes,
        "source": activity.source,
        "device_data": activity.device_data,
        "created_at": activity.created_at,
        "updated_at": activity.updated_at,
    })))
}

pub async fn update_activity(
    State((pool, _)): State<AppState>,
    Extension(user): Extension<User>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateActivityRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let updated = sqlx::query(
        r#"
        UPDATE activities SET
            activity_type  = COALESCE($1, activity_type),
            started_at     = COALESCE($2, started_at),
            duration_secs  = COALESCE($3, duration_secs),
            distance_meters = COALESCE($4, distance_meters),
            calories       = COALESCE($5, calories),
            notes          = COALESCE($6, notes),
            updated_at     = now()
        WHERE id = $7 AND user_id = $8
        "#,
    )
    .bind(body.activity_type)
    .bind(body.started_at)
    .bind(body.duration_secs)
    .bind(body.distance_meters)
    .bind(body.calories)
    .bind(body.notes)
    .bind(id)
    .bind(user.id)
    .execute(&pool)
    .await?;

    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    Ok(Json(serde_json::json!({ "message": "Activity updated" })))
}

pub async fn delete_activity(
    State((pool, _)): State<AppState>,
    Extension(user): Extension<User>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let deleted = sqlx::query("DELETE FROM activities WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user.id)
        .execute(&pool)
        .await?;

    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    Ok(Json(serde_json::json!({ "message": "Activity deleted" })))
}
