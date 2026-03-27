use thiserror::Error;

#[derive(Debug, Error)]
pub enum NanitError {
    #[error("Authentication failed (status {status}): {message}")]
    AuthFailed { status: u16, message: String },

    #[error("MFA authentication required (phone: ...{phone_suffix})")]
    MfaRequired {
        mfa_token: String,
        phone_suffix: String,
        channel: String,
    },

    #[error("Refresh token has expired. Re-login required.")]
    ExpiredRefreshToken,

    #[error("WebSocket is not connected")]
    NotConnected,

    #[error("Request timed out")]
    RequestTimeout,

    #[error("WebSocket closed: {0}")]
    WebSocketClosed(String),

    #[error(transparent)]
    Http(#[from] reqwest::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Protobuf(#[from] prost::DecodeError),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
