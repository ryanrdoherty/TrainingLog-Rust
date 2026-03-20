use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub user_id: Uuid,
    pub display_name: Option<String>,
    pub preferred_units: String,
    pub phone_number: Option<String>,
    pub phone_verified: bool,
    pub preferences: Value,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProfileRequest {
    pub display_name: Option<String>,
    pub preferred_units: Option<String>,
    pub phone_number: Option<String>,
    pub preferences: Option<Value>,
}
