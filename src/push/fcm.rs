use std::time::{Duration, Instant};

use async_trait::async_trait;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

const FCM_SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GRANT_TYPE: &str = "urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendOutcome {
    Delivered,
    Unregistered,
}

#[derive(Debug)]
pub enum FcmSendError {
    Transient,
    Permanent,
}

#[async_trait]
pub trait FcmSender: Send + Sync {
    async fn send(
        &self,
        _token: &str,
        notification_id: Uuid,
        title: &str,
        body: &str,
    ) -> Result<SendOutcome, FcmSendError>;
}

pub struct DisabledSender;

#[async_trait]
impl FcmSender for DisabledSender {
    async fn send(
        &self,
        _token: &str,
        _notification_id: Uuid,
        _title: &str,
        _body: &str,
    ) -> Result<SendOutcome, FcmSendError> {
        Err(FcmSendError::Permanent)
    }
}

#[derive(Clone, Deserialize)]
struct ServiceAccountFile {
    client_email: String,
    private_key: String,
    #[serde(default)]
    private_key_id: Option<String>,
    #[serde(default)]
    token_uri: Option<String>,
    #[serde(default)]
    project_id: Option<String>,
}

#[derive(Serialize)]
struct GoogleJwtClaims {
    iss: String,
    scope: String,
    aud: String,
    iat: i64,
    exp: i64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: u64,
}

pub struct FcmHttpSender {
    project_id: String,
    client_email: String,
    encoding_key: EncodingKey,
    kid: Option<String>,
    token_uri: String,
    send_url_override: Option<String>,
    http: reqwest::Client,
    cached: Mutex<Option<(String, Instant)>>,
}

impl FcmHttpSender {
    pub fn from_service_account_json(
        json: &str,
        project_id: &str,
        send_url_override: Option<String>,
    ) -> Result<Self, String> {
        let parsed: ServiceAccountFile =
            serde_json::from_str(json).map_err(|_| "invalid service account JSON".to_string())?;
        let encoding_key = EncodingKey::from_rsa_pem(parsed.private_key.as_bytes())
            .map_err(|_| "invalid service account private key".to_string())?;
        let resolved_project = if project_id.is_empty() {
            parsed
                .project_id
                .ok_or_else(|| "FCM_PROJECT_ID is required".to_string())?
        } else {
            project_id.to_string()
        };
        Ok(Self {
            project_id: resolved_project,
            client_email: parsed.client_email,
            encoding_key,
            kid: parsed.private_key_id,
            token_uri: parsed.token_uri.unwrap_or_else(|| TOKEN_URL.to_string()),
            send_url_override,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .map_err(|_| "failed to build HTTP client".to_string())?,
            cached: Mutex::new(None),
        })
    }

    fn send_url(&self) -> String {
        if let Some(url) = &self.send_url_override {
            return url.clone();
        }
        format!(
            "https://fcm.googleapis.com/v1/projects/{}/messages:send",
            self.project_id
        )
    }

    async fn access_token(&self) -> Result<String, FcmSendError> {
        {
            let guard = self.cached.lock().await;
            if let Some((token, expiry)) = guard.as_ref() {
                if Instant::now() + Duration::from_secs(60) < *expiry {
                    return Ok(token.clone());
                }
            }
        }
        let now = chrono::Utc::now().timestamp();
        let claims = GoogleJwtClaims {
            iss: self.client_email.clone(),
            scope: FCM_SCOPE.to_string(),
            aud: self.token_uri.clone(),
            iat: now,
            exp: now + 3600,
        };
        let mut header = Header::new(Algorithm::RS256);
        if let Some(kid) = &self.kid {
            header.kid = Some(kid.clone());
        }
        let assertion = encode(&header, &claims, &self.encoding_key).map_err(|_| {
            tracing::error!("fcm service account jwt encode failed");
            FcmSendError::Permanent
        })?;
        let body = format!("grant_type={GRANT_TYPE}&assertion={assertion}");
        let response = self
            .http
            .post(&self.token_uri)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .map_err(|_| FcmSendError::Transient)?;
        if !response.status().is_success() {
            tracing::warn!(status = %response.status(), "fcm oauth token request failed");
            return Err(
                if response.status().is_server_error() || response.status().as_u16() == 429 {
                    FcmSendError::Transient
                } else {
                    FcmSendError::Permanent
                },
            );
        }
        let parsed: TokenResponse = response.json().await.map_err(|_| FcmSendError::Transient)?;
        let ttl = if parsed.expires_in == 0 {
            3600
        } else {
            parsed.expires_in
        };
        let expiry = Instant::now() + Duration::from_secs(ttl.saturating_sub(60).max(30));
        *self.cached.lock().await = Some((parsed.access_token.clone(), expiry));
        Ok(parsed.access_token)
    }
}

#[async_trait]
impl FcmSender for FcmHttpSender {
    async fn send(
        &self,
        token: &str,
        notification_id: Uuid,
        title: &str,
        body: &str,
    ) -> Result<SendOutcome, FcmSendError> {
        let access = self.access_token().await?;
        let payload = serde_json::json!({
            "message": {
                "token": token,
                "notification": { "title": title, "body": body },
                "data": { "notificationId": notification_id.to_string() },
                "android": {
                    "priority": "HIGH",
                    "notification": { "channel_id": "studysync_alerts" }
                },
                "apns": {
                    "payload": {
                        "aps": { "sound": "default" }
                    }
                }
            }
        });
        let bytes = serde_json::to_vec(&payload).map_err(|_| FcmSendError::Permanent)?;
        let response = self
            .http
            .post(self.send_url())
            .bearer_auth(access)
            .header("content-type", "application/json")
            .body(bytes)
            .send()
            .await
            .map_err(|_| FcmSendError::Transient)?;
        let status = response.status();
        if status.is_success() {
            return Ok(SendOutcome::Delivered);
        }
        let text = response.text().await.unwrap_or_default();
        let unregistered = status.as_u16() == 404
            || text.contains("UNREGISTERED")
            || text.contains("INVALID_ARGUMENT");
        if unregistered {
            tracing::info!(
                notification_id = %notification_id,
                "fcm token rejected; device will be revoked"
            );
            return Ok(SendOutcome::Unregistered);
        }
        if status.as_u16() == 429 || status.is_server_error() {
            tracing::warn!(status = %status, "fcm send transient failure");
            return Err(FcmSendError::Transient);
        }
        tracing::warn!(status = %status, "fcm send permanent failure");
        Err(FcmSendError::Permanent)
    }
}
