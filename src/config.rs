use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub jwt_expiry_hours: i64,

    pub google_client_id: String,
    pub google_client_secret: String,

    pub facebook_client_id: String,
    pub facebook_client_secret: String,

    pub app_base_url: String,

    pub smtp_host: Option<String>,
    pub smtp_port: u16,
    pub smtp_user: Option<String>,
    pub smtp_pass: Option<String>,
    pub smtp_from: Option<String>,

    pub twilio_account_sid: Option<String>,
    pub twilio_auth_token: Option<String>,
    pub twilio_from_number: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            database_url: required("DATABASE_URL")?,
            jwt_secret: required("JWT_SECRET")?,
            jwt_expiry_hours: std::env::var("JWT_EXPIRY_HOURS")
                .unwrap_or_else(|_| "24".into())
                .parse()
                .context("JWT_EXPIRY_HOURS must be an integer")?,

            google_client_id: required("GOOGLE_CLIENT_ID")?,
            google_client_secret: required("GOOGLE_CLIENT_SECRET")?,

            facebook_client_id: required("FACEBOOK_CLIENT_ID")?,
            facebook_client_secret: required("FACEBOOK_CLIENT_SECRET")?,

            app_base_url: required("APP_BASE_URL")?,

            smtp_host: optional("SMTP_HOST"),
            smtp_port: std::env::var("SMTP_PORT")
                .unwrap_or_else(|_| "587".into())
                .parse()
                .context("SMTP_PORT must be an integer")?,
            smtp_user: optional("SMTP_USER"),
            smtp_pass: optional("SMTP_PASS"),
            smtp_from: optional("SMTP_FROM"),

            twilio_account_sid: optional("TWILIO_ACCOUNT_SID"),
            twilio_auth_token: optional("TWILIO_AUTH_TOKEN"),
            twilio_from_number: optional("TWILIO_FROM_NUMBER"),
        })
    }
}

fn required(key: &str) -> Result<String> {
    std::env::var(key).with_context(|| format!("Missing required env var: {key}"))
}

fn optional(key: &str) -> Option<String> {
    std::env::var(key).ok()
}
