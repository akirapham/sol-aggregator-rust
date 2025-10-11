use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose};
use std::sync::Arc;

#[derive(Clone)]
pub struct AuthConfig {
    pub username: String,
    pub password: String,
}

impl AuthConfig {
    pub fn from_env() -> Self {
        let username = std::env::var("DASHBOARD_USERNAME")
            .unwrap_or_else(|_| "admin".to_string());
        let password = std::env::var("DASHBOARD_PASSWORD")
            .unwrap_or_else(|_| "password".to_string());

        log::info!("Dashboard auth configured for user: {}", username);

        Self { username, password }
    }
}

/// Middleware to check HTTP Basic Auth
pub async fn auth_middleware(
    State(config): State<Arc<AuthConfig>>,
    request: Request,
    next: Next,
) -> Response {
    // Get the Authorization header
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok());

    if let Some(auth_value) = auth_header {
        if auth_value.starts_with("Basic ") {
            let encoded = &auth_value[6..];
            if let Ok(decoded) = general_purpose::STANDARD.decode(encoded) {
                if let Ok(credentials) = String::from_utf8(decoded) {
                    let parts: Vec<&str> = credentials.splitn(2, ':').collect();
                    if parts.len() == 2 {
                        let username = parts[0];
                        let password = parts[1];

                        if username == config.username && password == config.password {
                            // Authentication successful
                            return next.run(request).await;
                        }
                    }
                }
            }
        }
    }

    // Authentication failed - return 401
    let body = "Unauthorized";
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Basic realm=\"Dashboard\"")],
        body,
    )
        .into_response()
}
