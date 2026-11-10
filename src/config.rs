//! Credentials and settings, resolved once at startup.
//!
//! Precedence, first hit wins:
//!   1. the real environment (`QUAD_TOKEN`, a `QUAD_*` variable)
//!   2. a `.env` next to the working directory (then the home config dir)
//!   3. `~/.config/quad/config.toml` (written by `quad login`)
//!
//! The portal origin can be derived from the token itself (the `instituteUrl`
//! claim), so a token on its own is enough to run. That keeps onboarding to a
//! single secret.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::error::{QuadError, jwt_claims};

pub const DEFAULT_TIMEZONE: &str = "Asia/Kolkata";
pub const DEFAULT_TIMEOUT_MS: u64 = 40_000;

#[derive(Debug, Clone)]
pub struct Config {
    pub token: String,
    pub base_url: String,
    pub timezone: String,
    pub timeout_ms: u64,
    /// uid of the signed-in student, read from the token when present.
    pub ukid: Option<i64>,
    /// Expiry as a unix timestamp (seconds), read from the token when present.
    pub expires_at: Option<i64>,
    pub college: Option<String>,
}

impl Config {
    /// True when the token's `exp` has passed (60 s grace for clock skew).
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(exp) => exp + 60 < now_unix(),
            None => false,
        }
    }

    /// Whole days until the token expires, negative once it has.
    pub fn days_left(&self) -> Option<i64> {
        self.expires_at
            .map(|exp| (exp - now_unix()).div_euclid(86_400))
    }
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

pub fn config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("QUAD_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    home().map(|h| h.join(".config").join("quad"))
}

pub fn config_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("config.toml"))
}

// ------------------------------------------------------------------------- //
// Minimal .env / config.toml readers
//
// Both files are tiny and only ever hold `KEY = "value"` pairs, so a full
// parser (and its dependency) would be dead weight.
// ------------------------------------------------------------------------- //

/// Parse `KEY=value` lines, tolerating `export`, quotes and `#` comments.
pub fn parse_env(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        out.insert(key.to_string(), unquote(value.trim()));
    }
    out
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    for q in ['"', '\''] {
        if value.len() >= 2 && value.starts_with(q) && value.ends_with(q) {
            return value[1..value.len() - 1].to_string();
        }
    }
    // strip an unquoted trailing comment
    match value.split_once(" #") {
        Some((head, _)) => head.trim_end().to_string(),
        None => value.to_string(),
    }
}

/// Parse the flat `key = "value"` subset of TOML that `quad login` writes.
/// Only the section-less head of the file is read; unknown tables are ignored.
pub fn parse_toml_flat(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut in_table = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            // Only the section-less head of the file holds our keys.
            in_table = true;
            continue;
        }
        if in_table {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        out.insert(key.trim().to_string(), unquote(value.trim()));
    }
    out
}

fn read_if_exists(path: &PathBuf) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// Layered lookup: real env → .env files → config.toml.
fn lookup(key: &str) -> Option<String> {
    if let Ok(v) = std::env::var(key)
        && !v.trim().is_empty()
    {
        return Some(v.trim().to_string());
    }
    let mut candidates: Vec<PathBuf> = vec![PathBuf::from(".env")];
    if let Some(dir) = config_dir() {
        candidates.push(dir.join(".env"));
        candidates.push(dir.join("config.toml"));
    }
    if let Some(h) = home() {
        candidates.push(h.join(".env"));
    }
    for path in candidates {
        let Some(text) = read_if_exists(&path) else {
            continue;
        };
        let parsed = if path.extension().is_some_and(|e| e == "toml") {
            parse_toml_flat(&text)
        } else {
            parse_env(&text)
        };
        let lower = key.to_lowercase();
        let stripped = key.strip_prefix("QUAD_").map(|s| s.to_lowercase());
        let hit = parsed
            .get(key)
            .or_else(|| parsed.get(&lower))
            .or_else(|| stripped.as_deref().and_then(|s| parsed.get(s)));
        if let Some(v) = hit
            && !v.trim().is_empty()
        {
            return Some(v.trim().to_string());
        }
    }
    None
}

fn normalise_origin(raw: &str) -> String {
    let raw = raw.trim().trim_end_matches('/');
    if raw.starts_with("http://") || raw.starts_with("https://") {
        raw.to_string()
    } else {
        format!("https://{raw}")
    }
}

/// Resolve configuration. `token` may be passed explicitly (CLI flag).
pub fn load(token_override: Option<String>) -> Result<Config, QuadError> {
    let token = token_override
        .or_else(|| lookup("QUAD_TOKEN"))
        .or_else(|| lookup("DIGII_TOKEN"))
        .ok_or_else(|| {
            QuadError::Config(
                "No token found. Run `quad login --portal <institute>.digiicampus.com` \
                 (it opens a browser and finds the token for you), or set QUAD_TOKEN."
                    .into(),
            )
        })?;

    let claims = jwt_claims(&token).unwrap_or(serde_json::Value::Null);
    let claim = |k: &str| claims.get(k).and_then(|v| v.as_str()).map(str::to_string);

    let base_url = lookup("QUAD_BASE_URL")
        .or_else(|| lookup("DIGII_BASE_URL"))
        .filter(|s| !s.trim().is_empty())
        .or_else(|| claim("instituteUrl"))
        .map(|s| normalise_origin(&s))
        .ok_or_else(|| {
            QuadError::Config(
                "The portal origin is unknown: set QUAD_BASE_URL to your institution origin, \
                 e.g. https://<institute>.digiicampus.com"
                    .into(),
            )
        })?;

    let timezone = lookup("QUAD_TIMEZONE")
        .or_else(|| claim("timeZone"))
        .unwrap_or_else(|| DEFAULT_TIMEZONE.to_string());

    let timeout_ms = match lookup("QUAD_TIMEOUT_MS") {
        Some(raw) => raw.parse::<u64>().ok().filter(|n| *n > 0).ok_or_else(|| {
            QuadError::Config(format!(
                "QUAD_TIMEOUT_MS must be a positive number, got {raw:?}"
            ))
        })?,
        None => DEFAULT_TIMEOUT_MS,
    };

    Ok(Config {
        ukid: claims.get("ukid").and_then(|v| v.as_i64()),
        expires_at: claims.get("exp").and_then(|v| v.as_i64()),
        college: claim("collegeName").or_else(|| claim("tenantName")),
        token,
        base_url,
        timezone,
        timeout_ms,
    })
}

/// Persist a token to `~/.config/quad/config.toml` (chmod 600).
pub fn save_token(token: &str, base_url: &str) -> Result<PathBuf, QuadError> {
    let dir = config_dir()
        .ok_or_else(|| QuadError::Config("cannot locate a config directory (HOME unset)".into()))?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| QuadError::Config(format!("cannot create {}: {e}", dir.display())))?;
    let path = dir.join("config.toml");
    let body = format!(
        "# quad — written by `quad login`. One student, one portal.\n\
         # This file grants full access to your record: keep it private.\n\
         token = \"{token}\"\n\
         base_url = \"{base_url}\"\n"
    );
    std::fs::write(&path, body)
        .map_err(|e| QuadError::Config(format!("cannot write {}: {e}", path.display())))?;
    restrict_permissions(&path);
    Ok(path)
}

#[cfg(unix)]
fn restrict_permissions(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &std::path::Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_env_lines() {
        let env = parse_env(
            "# c\nexport QUAD_TOKEN=\"abc\"\nQUAD_BASE_URL=https://x.example.com\nEMPTY=\nBAD\n",
        );
        assert_eq!(env.get("QUAD_TOKEN").unwrap(), "abc");
        assert_eq!(env.get("QUAD_BASE_URL").unwrap(), "https://x.example.com");
        assert_eq!(env.get("EMPTY").unwrap(), "");
        assert!(!env.contains_key("BAD"));
    }

    #[test]
    fn parses_toml_flat_and_skips_tables() {
        let t =
            parse_toml_flat("token = \"a.b.c\"\nbase_url = 'https://x'\n[mcp]\nfoo = \"bar\"\n");
        assert_eq!(t.get("token").unwrap(), "a.b.c");
        assert_eq!(t.get("base_url").unwrap(), "https://x");
        assert!(!t.contains_key("foo"));
    }

    #[test]
    fn normalises_origins() {
        assert_eq!(normalise_origin("a.example.com/"), "https://a.example.com");
        assert_eq!(
            normalise_origin("http://a.example.com"),
            "http://a.example.com"
        );
    }
}
