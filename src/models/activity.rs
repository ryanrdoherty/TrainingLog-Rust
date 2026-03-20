use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Activity {
    pub id: Uuid,
    pub user_id: Uuid,
    pub activity_type: String,
    pub started_at: DateTime<Utc>,
    pub duration_secs: i32,
    pub distance_meters: Option<f32>,
    pub calories: Option<i32>,
    pub notes: Option<String>,
    pub source: String,
    pub device_data: Option<Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateActivityRequest {
    pub activity_type: String,
    pub started_at: DateTime<Utc>,
    pub duration_secs: i32,
    pub distance_meters: Option<f32>,
    pub calories: Option<i32>,
    pub notes: Option<String>,
    pub device_data: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateActivityRequest {
    pub activity_type: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub duration_secs: Option<i32>,
    pub distance_meters: Option<f32>,
    pub calories: Option<i32>,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ActivityQuery {
    pub activity_type: Option<String>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
