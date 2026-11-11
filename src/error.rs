//! Every failure the portal (or the transport) can produce, reduced to
//! messages an agent can act on and never rendering a secret.

use crate::portal::base64url;

/// Redact credentials, tokens, and signed URLs from error messages and snippets
/// before they can ever leak into the agent context or terminal logs.
pub fn redact_secrets(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        // Redact JWT tokens (starting with eyJ)
        if ch == 'e' && chars.clone().take(2).collect::<String>() == "yJ" {
            out.push_str("[REDACTED_JWT]");
            // Skip the rest of the base64url JWT token
            while let Some(&c) = chars.peek() {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                    chars.next();
                } else {
                    break;
                }
            }
            continue;
        }

        // Redact common sensitive query parameters
        let remaining: String = chars.clone().take(20).collect();
        if remaining.starts_with("Signature=")
            || remaining.starts_with("token=")
            || remaining.starts_with("Auth-Token=")
        {
            out.push(ch);
            while let Some(&c) = chars.peek() {
                out.push(c);
                chars.next();
                if c == '=' {
                    out.push_str("[REDACTED]");
                    while let Some(&val_char) = chars.peek() {
                        if val_char != '&' && val_char != ' ' && val_char != '"' && val_char != '\''
                        {
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    break;
                }
            }
            continue;
        }

        out.push(ch);
    }

    out
}

#[derive(Debug)]
pub enum DigiError {
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
    /// Form or write validation failed (e.g. coverage, ambiguous form).
    Validation(String),
}

pub type QuadError = DigiError;

impl std::fmt::Display for DigiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DigiError::Config(m) => write!(f, "{}", redact_secrets(m)),
            DigiError::Unauthorized => write!(
                f,
                "Session expired or missing — refresh the token (`digi login`, or update `DIGIMCP_TOKEN`)."
            ),
            DigiError::Forbidden(m) => write!(f, "Not permitted: {}", redact_secrets(m)),
            DigiError::NotFound(m) => write!(f, "Not found: {}", redact_secrets(m)),
            DigiError::Network(m) => {
                write!(f, "Network unavailable or timed out: {}", redact_secrets(m))
            }
            DigiError::SpaLoop(m) => write!(
                f,
                "The portal returned a web page instead of data for {} — the private route likely changed.",
                redact_secrets(m)
            ),
            DigiError::Http { status, body } => {
                let redacted = redact_secrets(body);
                let snippet = redacted.chars().take(160).collect::<String>();
                write!(f, "Portal error {status}: {snippet}")
            }
            DigiError::Parse(m) => write!(
                f,
                "Unreadable response from the portal: {}",
                redact_secrets(m)
            ),
            DigiError::Validation(m) => write!(f, "Validation error: {}", redact_secrets(m)),
        }
    }
}

impl std::error::Error for DigiError {}

impl From<reqwest::Error> for DigiError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            DigiError::Network("request timed out".into())
        } else if e.is_connect() {
            DigiError::Network("could not connect".into())
        } else if e.is_decode() {
            DigiError::Parse(e.to_string())
        } else {
            DigiError::Network(e.to_string())
        }
    }
}

/// Decode a JWT payload without checking the signature — we only need the
/// claims the portal itself issued to us.
pub fn jwt_claims(token: &str) -> Result<serde_json::Value, DigiError> {
    let payload = token.split('.').nth(1).ok_or_else(|| {
        DigiError::Config("the token is not a JWT (expected 3 dot-separated parts)".into())
    })?;
    let bytes = base64url(payload)
        .ok_or_else(|| DigiError::Config("the token payload is not valid base64url".into()))?;
    serde_json::from_slice(&bytes)
        .map_err(|_| DigiError::Config("the token payload is not JSON".into()))
}
