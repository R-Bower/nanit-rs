use serde::{Deserialize, Serialize};

pub const API_BASE_URL: &str = "https://api.nanit.com";
pub const AUTH_TOKEN_LIFETIME_MS: u64 = 60 * 60 * 1000; // 60 minutes
pub const WS_BASE_URL: &str = "wss://api.nanit.com/focus/cameras";

// --- Baby ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baby {
    pub uid: String,
    pub name: String,
    pub camera_uid: String,
}

// --- Messages ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NanitMessage {
    pub id: i64,
    #[serde(rename = "type")]
    pub msg_type: String,
    pub baby_uid: String,
    pub time: i64, // Unix timestamp
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub seen_at: Option<String>,
    #[serde(default)]
    pub read_at: Option<String>,
    #[serde(default)]
    pub dismissed_at: Option<String>,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
    #[serde(default)]
    pub user_id: Option<i64>,
}

// --- Auth ---

#[derive(Debug, Serialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mfa_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mfa_code: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AuthResponse {
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Debug, Serialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Debug, Deserialize)]
pub struct MfaEnabledResponse {
    pub mfa_token: String,
    pub phone_suffix: String,
    pub channel: String,
}

// --- API Responses ---

#[derive(Debug, Deserialize)]
pub struct BabiesResponse {
    pub babies: Vec<Baby>,
}

#[derive(Debug, Deserialize)]
pub struct MessagesResponse {
    pub messages: Vec<NanitMessage>,
}
