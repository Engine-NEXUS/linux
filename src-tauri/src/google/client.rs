use super::types::GoogleError;
use reqwest::{header, Client, StatusCode};
use serde_json::Value;
use std::time::Duration;

/// HTTP client for Google REST APIs with automatic auth header and error mapping.
pub struct GoogleClient {
    http: Client,
    token: String,
}

impl GoogleClient {
    pub fn new(token: String) -> Result<Self, GoogleError> {
        let http = Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| GoogleError::NetworkError(e.to_string()))?;

        Ok(Self { http, token })
    }

    /// Helper to convert HTTP status codes into typed `GoogleError`
    pub fn map_status_code(status: StatusCode, error_body: &str) -> GoogleError {
        match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => GoogleError::AuthRequired,
            StatusCode::NOT_FOUND => GoogleError::NotFound(
                format!("Requested Google resource not found: {}", error_body),
            ),
            StatusCode::TOO_MANY_REQUESTS => GoogleError::RateLimited,
            _ => GoogleError::ApiError {
                code: status.as_u16(),
                message: error_body.to_string(),
            },
        }
    }

    /// Perform authenticated GET request returning parsed JSON Value.
    pub async fn get_json(&self, url: &str) -> Result<Value, GoogleError> {
        let resp = self
            .http
            .get(url)
            .header(header::AUTHORIZATION, format!("Bearer {}", self.token))
            .header(header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| GoogleError::NetworkError(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(Self::map_status_code(status, &body));
        }

        resp.json::<Value>()
            .await
            .map_err(|e| GoogleError::SerializationError(e.to_string()))
    }

    /// Perform authenticated POST request with JSON body.
    pub async fn post_json(&self, url: &str, body: &Value) -> Result<Value, GoogleError> {
        let resp = self
            .http
            .post(url)
            .header(header::AUTHORIZATION, format!("Bearer {}", self.token))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| GoogleError::NetworkError(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            let error_text = resp.text().await.unwrap_or_default();
            return Err(Self::map_status_code(status, &error_text));
        }

        resp.json::<Value>()
            .await
            .map_err(|e| GoogleError::SerializationError(e.to_string()))
    }
}

// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_code_mapping() {
        let err_unauth = GoogleClient::map_status_code(StatusCode::UNAUTHORIZED, "Invalid Credentials");
        assert_eq!(err_unauth, GoogleError::AuthRequired);

        let err_nf = GoogleClient::map_status_code(StatusCode::NOT_FOUND, "Thread not found");
        assert!(matches!(err_nf, GoogleError::NotFound(_)));

        let err_rl = GoogleClient::map_status_code(StatusCode::TOO_MANY_REQUESTS, "Quota exceeded");
        assert_eq!(err_rl, GoogleError::RateLimited);

        let err_500 = GoogleClient::map_status_code(StatusCode::INTERNAL_SERVER_ERROR, "Server Error");
        assert!(matches!(err_500, GoogleError::ApiError { code: 500, .. }));
    }
}
