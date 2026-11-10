//! Every failure the portal (or the transport) can produce, reduced to
//! messages an agent can act on and never rendering a secret.

use crate::portal::base64url;

#[derive(Debug)]
pub enum QuadError {
    /// No credentials configured, or they are unusable.
    Config(String),
    /// 401 — the `user` cookie expired or was revoked.
    Unauthorized,
    /// 403 or an app error code such as `CLASS_FACULTY_PRIVILEGE_REQUIRED`.
    Forbidden(String),
    /// 404 — the item is gone or the private route moved.
    NotFound(String),
    /// Transport failure or timeout.
    Network(String),
    /// The portal answered with HTML (SPA fallback for an unknown route).
    SpaLoop(String),
    /// Any other non-2xx.
    Http { status: u16, body: String },
    /// A response we could not read as the expected JSON.
    Parse(String),
}

impl std::fmt::Display for QuadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QuadError::Config(m) => write!(f, "{m}"),
            QuadError::Unauthorized => write!(
                f,
                "Session expired or missing — refresh the token (`quad login`, or update `QUAD_TOKEN`)."
            ),
            QuadError::Forbidden(m) => write!(f, "Not permitted: {m}"),
            QuadError::NotFound(m) => write!(f, "Not found: {m}"),
            QuadError::Network(m) => write!(f, "Network unavailable or timed out: {m}"),
            QuadError::SpaLoop(m) => write!(
                f,
                "The portal returned a web page instead of data for {m} — the private route likely changed."
            ),
            QuadError::Http { status, body } => {
                let snippet = body.chars().take(160).collect::<String>();
                write!(f, "Portal error {status}: {snippet}")
            }
            QuadError::Parse(m) => write!(f, "Unreadable response from the portal: {m}"),
        }
    }
}

impl std::error::Error for QuadError {}

impl From<reqwest::Error> for QuadError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            QuadError::Network("request timed out".into())
        } else if e.is_connect() {
            QuadError::Network("could not connect".into())
        } else if e.is_decode() {
            QuadError::Parse(e.to_string())
        } else {
            QuadError::Network(e.to_string())
        }
    }
}

/// Decode a JWT payload without checking the signature — we only need the
/// claims the portal itself issued to us.
pub fn jwt_claims(token: &str) -> Result<serde_json::Value, QuadError> {
    let payload = token.split('.').nth(1).ok_or_else(|| {
        QuadError::Config("the token is not a JWT (expected 3 dot-separated parts)".into())
    })?;
    let bytes = base64url(payload)
        .ok_or_else(|| QuadError::Config("the token payload is not valid base64url".into()))?;
    serde_json::from_slice(&bytes)
        .map_err(|_| QuadError::Config("the token payload is not JSON".into()))
}
