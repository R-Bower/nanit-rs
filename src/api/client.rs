use chrono::Utc;
use reqwest::Client;
use tracing::{debug, warn};

use super::error::NanitError;
use super::types::*;
use crate::session::SessionStore;

const HTTP_STATUS_CREATED: u16 = 201;
const HTTP_STATUS_UNAUTHORIZED: u16 = 401;
const HTTP_STATUS_NOT_FOUND: u16 = 404;
const HTTP_STATUS_MFA_REQUIRED: u16 = 482;
const MAX_AUTH_RETRIES: usize = 2;

pub struct NanitClient {
    http: Client,
    base_url: String,
}

impl NanitClient {
    pub fn new() -> Self {
        Self {
            http: Client::new(),
            base_url: API_BASE_URL.to_string(),
        }
    }

    #[cfg(test)]
    pub fn with_base_url(base_url: &str) -> Self {
        Self {
            http: Client::new(),
            base_url: base_url.to_string(),
        }
    }

    /// Login with email and password.
    pub async fn login(
        &self,
        session: &mut SessionStore,
        email: &str,
        password: &str,
    ) -> Result<AuthResponse, NanitError> {
        self.do_login(
            session,
            &LoginRequest {
                email: email.to_string(),
                password: password.to_string(),
                mfa_token: None,
                mfa_code: None,
            },
        )
        .await
    }

    /// Complete login with MFA code.
    pub async fn login_with_mfa(
        &self,
        session: &mut SessionStore,
        email: &str,
        password: &str,
        mfa_token: &str,
        mfa_code: &str,
    ) -> Result<AuthResponse, NanitError> {
        self.do_login(
            session,
            &LoginRequest {
                email: email.to_string(),
                password: password.to_string(),
                mfa_token: Some(mfa_token.to_string()),
                mfa_code: Some(mfa_code.to_string()),
            },
        )
        .await
    }

    /// Renew session using stored refresh token.
    pub async fn renew_session(&self, session: &mut SessionStore) -> Result<(), NanitError> {
        let refresh_token = session.refresh_token().to_string();
        if refresh_token.is_empty() {
            return Err(NanitError::ExpiredRefreshToken);
        }

        let response = self
            .http
            .post(format!("{}/tokens/refresh", self.base_url))
            .json(&RefreshRequest {
                refresh_token: refresh_token.clone(),
            })
            .send()
            .await?;

        let status = response.status().as_u16();

        if status == HTTP_STATUS_NOT_FOUND {
            return Err(NanitError::ExpiredRefreshToken);
        }

        if !response.status().is_success() {
            return Err(NanitError::AuthFailed {
                status,
                message: format!("Token refresh failed with status {status}"),
            });
        }

        let data: AuthResponse = response.json().await?;
        session.set_auth_token(&data.access_token);
        session.set_refresh_token(&data.refresh_token);
        session.set_auth_time(Utc::now());
        let _ = session.save();

        Ok(())
    }

    /// Ensure we have a valid auth token, refreshing if needed.
    pub async fn maybe_authorize(
        &self,
        session: &mut SessionStore,
        force: bool,
    ) -> Result<(), NanitError> {
        if force
            || session.auth_token().is_empty()
            || session.is_token_expired(AUTH_TOKEN_LIFETIME_MS)
        {
            debug!("Refreshing auth token");
            self.renew_session(session).await?;
        }
        Ok(())
    }

    /// Fetch the list of babies associated with the account.
    pub async fn fetch_babies(
        &self,
        session: &mut SessionStore,
    ) -> Result<Vec<Baby>, NanitError> {
        let data: BabiesResponse = self
            .fetch_authorized(session, &format!("{}/babies", self.base_url))
            .await?;
        session.set_babies(data.babies.clone());
        let _ = session.save();
        Ok(data.babies)
    }

    /// Fetch event messages for a baby.
    pub async fn fetch_messages(
        &self,
        session: &mut SessionStore,
        baby_uid: &str,
        limit: u32,
    ) -> Result<Vec<NanitMessage>, NanitError> {
        let data: MessagesResponse = self
            .fetch_authorized(
                session,
                &format!("{}/babies/{}/messages?limit={}", self.base_url, baby_uid, limit),
            )
            .await?;
        Ok(data.messages)
    }

    /// Ensure babies are loaded, fetching if needed.
    pub async fn ensure_babies(
        &self,
        session: &mut SessionStore,
    ) -> Result<Vec<Baby>, NanitError> {
        if session.babies().is_empty() {
            return self.fetch_babies(session).await;
        }
        Ok(session.babies().to_vec())
    }

    // --- Private ---

    async fn do_login(
        &self,
        session: &mut SessionStore,
        req: &LoginRequest,
    ) -> Result<AuthResponse, NanitError> {
        let response = self
            .http
            .post(format!("{}/login", self.base_url))
            .header("Content-Type", "application/json")
            .header("nanit-api-version", "2")
            .json(req)
            .send()
            .await?;

        let status = response.status().as_u16();

        if status == HTTP_STATUS_UNAUTHORIZED {
            return Err(NanitError::AuthFailed {
                status: HTTP_STATUS_UNAUTHORIZED,
                message: "Provided credentials were not accepted by the server".to_string(),
            });
        }

        if status == HTTP_STATUS_MFA_REQUIRED {
            let mfa_data: MfaEnabledResponse = response.json().await?;
            return Err(NanitError::MfaRequired {
                mfa_token: mfa_data.mfa_token,
                phone_suffix: mfa_data.phone_suffix,
                channel: mfa_data.channel,
            });
        }

        if status != HTTP_STATUS_CREATED {
            return Err(NanitError::AuthFailed {
                status,
                message: format!("Login failed with unexpected status {status}"),
            });
        }

        let data: AuthResponse = response.json().await?;
        session.set_auth_token(&data.access_token);
        session.set_refresh_token(&data.refresh_token);
        session.set_auth_time(Utc::now());
        let _ = session.save();

        Ok(data)
    }

    /// Make an authorized request, auto-retrying once on 401.
    async fn fetch_authorized<T: serde::de::DeserializeOwned>(
        &self,
        session: &mut SessionStore,
        url: &str,
    ) -> Result<T, NanitError> {
        for attempt in 0..MAX_AUTH_RETRIES {
            self.maybe_authorize(session, attempt > 0).await?;

            let token = session.auth_token().to_string();
            let response = self
                .http
                .get(url)
                // REST: Authorization: {token} — NO Bearer prefix
                .header("Authorization", &token)
                .send()
                .await?;

            let status = response.status().as_u16();

            if status == HTTP_STATUS_UNAUTHORIZED {
                warn!("Got 401, will force-refresh on next attempt");
                continue;
            }

            if !response.status().is_success() {
                return Err(NanitError::AuthFailed {
                    status,
                    message: format!("Request to {url} failed with status {status}"),
                });
            }

            return Ok(response.json().await?);
        }

        Err(NanitError::AuthFailed {
            status: HTTP_STATUS_UNAUTHORIZED,
            message: format!("Authorization failed after {MAX_AUTH_RETRIES} attempts"),
        })
    }
}
