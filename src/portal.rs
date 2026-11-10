//! The portal transport: one authenticated HTTP client, timezone-correct
//! date parsing, and the handful of read routes the tools need.
//!
//! Human architecture notes:
//! The Digiicampus (CollPoll) API is private, undocumented, and full of historical
//! quirks that trip up naive integrations:
//!
//!   1. **Auth Header Convention**: CollPoll ignores `Authorization: Bearer <token>`
//!      on almost all student endpoints; it strictly expects `Auth-Token: <jwt>`.
//!      Additionally, some tenant firewalls reject requests that lack a matching
//!      `Referer: <college-origin>/` header, treating them as potential cross-origin
//!      forgery attacks. We configure both in `Portal::new`.
//!
//!   2. **Silent Permission Failures**: When a student token accesses a faculty-only
//!      or admin-only route, the backend doesn't send a helpful JSON error payload — it
//!      often responds with a blank 403 or redirects to the Single Page Application's
//!      HTML login shell. Our transport detects this (`is_html` check) and wraps it
//!      in [`crate::error::QuadError::SpaLoop`], giving the calling AI agent an actionable
//!      explanation rather than an unparseable JSON error.
//!
//!   3. **Naive Datetime Trap**: Deadlines in the database are stored as naive local
//!      timestamp strings (e.g. `"2026-11-10 23:59:00"`). If an agent running on a laptop
//!      in UTC or US Pacific parses this naively, every single assignment deadline shifts
//!      by 5.5 to 13.5 hours! We anchor all naive datetimes to the institution's IANA
//!      timezone (defaulting to Asia/Kolkata) and convert to RFC 3339 with calculated
//!      relative `hoursUntilDue`.

use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::{Value, json};

use crate::config::Config;
use crate::error::QuadError;

/// base64url decode without padding, for JWT segments.
pub fn base64url(input: &str) -> Option<Vec<u8>> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = Vec::with_capacity(input.len() * 3 / 4 + 3);
    let mut buf: u32 = 0;
    let mut bits = 0u32;
    for byte in input.bytes() {
        if byte == b'=' || byte.is_ascii_whitespace() {
            continue;
        }
        let val = TABLE.iter().position(|&c| c == byte)? as u32;
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

#[derive(Clone)]
pub struct Portal {
    http: reqwest::Client,
    cfg: Config,
}

impl Portal {
    pub fn new(cfg: Config) -> Result<Self, QuadError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "Accept",
            HeaderValue::from_static("application/json, text/plain, */*"),
        );
        headers.insert(
            "Auth-Token",
            HeaderValue::from_str(&cfg.token).map_err(|_| {
                QuadError::Config(
                    "the token contains characters that cannot be sent in a header".into(),
                )
            })?,
        );
        if let Ok(referer) = HeaderValue::from_str(&format!("{}/", cfg.base_url)) {
            headers.insert("Referer", referer);
        }
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_millis(cfg.timeout_ms))
            .user_agent("quad (+https://github.com/kushvinth/quad)")
            .build()?;
        Ok(Self { http, cfg })
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    /// GET a JSON route, mapping every failure onto an actionable error.
    pub async fn get(&self, path: &str, query: &[(&str, QueryArg)]) -> Result<Value, QuadError> {
        let mut url = format!("{}{}", self.cfg.base_url, path);
        let pairs: Vec<String> = query.iter().filter_map(|(k, v)| v.pair(k)).collect();
        if !pairs.is_empty() {
            url.push('?');
            url.push_str(&pairs.join("&"));
        }

        let res = self.http.get(&url).send().await.map_err(|e| {
            if e.status() == Some(reqwest::StatusCode::UNAUTHORIZED) {
                QuadError::Unauthorized
            } else {
                QuadError::from(e)
            }
        })?;

        let status = res.status();
        let text = res.text().await.map_err(QuadError::from)?;

        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(QuadError::Unauthorized);
        }
        let trimmed = text.trim_start();
        let is_html = trimmed.len() >= 9
            && (trimmed[..9].to_ascii_lowercase().contains("<!doctype")
                || trimmed[..trimmed.len().min(5)].eq_ignore_ascii_case("<html"));
        if !status.is_success() {
            let body = serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
            let code = body.get("error").and_then(Value::as_str).unwrap_or("");
            let message = body
                .get("message")
                .and_then(Value::as_str)
                .filter(|m| !m.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| code.to_string());
            return match status.as_u16() {
                403 => Err(QuadError::Forbidden(if message.is_empty() {
                    "this route is not available to student accounts".into()
                } else {
                    message
                })),
                404 => Err(QuadError::NotFound(path.to_string())),
                _ if code.contains("PRIVILEGE") || code.contains("NOT_ALLOWED") => {
                    Err(QuadError::Forbidden(message))
                }
                other => Err(QuadError::Http {
                    status: other,
                    body: text,
                }),
            };
        }
        if is_html {
            return Err(QuadError::SpaLoop(path.to_string()));
        }
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).map_err(|e| QuadError::Parse(format!("{e} (in {path})")))
    }

    /// POST a JSON route, mapping every failure onto an actionable error.
    pub async fn post(&self, path: &str, body: &Value) -> Result<Value, QuadError> {
        let url = format!("{}{}", self.cfg.base_url, path);
        let res = self.http.post(&url).json(body).send().await.map_err(|e| {
            if e.status() == Some(reqwest::StatusCode::UNAUTHORIZED) {
                QuadError::Unauthorized
            } else {
                QuadError::from(e)
            }
        })?;

        let status = res.status();
        let text = res.text().await.map_err(QuadError::from)?;

        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(QuadError::Unauthorized);
        }
        let trimmed = text.trim_start();
        let is_html = trimmed.len() >= 9
            && (trimmed[..9].to_ascii_lowercase().contains("<!doctype")
                || trimmed[..trimmed.len().min(5)].eq_ignore_ascii_case("<html"));
        if !status.is_success() {
            let body = serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
            let code = body.get("error").and_then(Value::as_str).unwrap_or("");
            let message = body
                .get("message")
                .and_then(Value::as_str)
                .filter(|m| !m.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| code.to_string());
            return match status.as_u16() {
                403 => Err(QuadError::Forbidden(if message.is_empty() {
                    "this action is not permitted for your student account".into()
                } else {
                    message
                })),
                404 => Err(QuadError::NotFound(path.to_string())),
                _ if code.contains("PRIVILEGE") || code.contains("NOT_ALLOWED") => {
                    Err(QuadError::Forbidden(message))
                }
                other => Err(QuadError::Http {
                    status: other,
                    body: text,
                }),
            };
        }
        if is_html {
            return Err(QuadError::SpaLoop(path.to_string()));
        }
        if text.trim().is_empty() {
            return Ok(json!({ "success": true }));
        }
        Ok(serde_json::from_str(&text).unwrap_or_else(|_| json!({ "status": "ok", "raw": text })))
    }

    /// PUT a JSON route, mapping every failure onto an actionable error.
    #[allow(dead_code)]
    pub async fn put(&self, path: &str, body: &Value) -> Result<Value, QuadError> {
        let url = format!("{}{}", self.cfg.base_url, path);
        let res = self.http.put(&url).json(body).send().await.map_err(|e| {
            if e.status() == Some(reqwest::StatusCode::UNAUTHORIZED) {
                QuadError::Unauthorized
            } else {
                QuadError::from(e)
            }
        })?;

        let status = res.status();
        let text = res.text().await.map_err(QuadError::from)?;

        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(QuadError::Unauthorized);
        }
        let trimmed = text.trim_start();
        let is_html = trimmed.len() >= 9
            && (trimmed[..9].to_ascii_lowercase().contains("<!doctype")
                || trimmed[..trimmed.len().min(5)].eq_ignore_ascii_case("<html"));
        if !status.is_success() {
            let body = serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
            let code = body.get("error").and_then(Value::as_str).unwrap_or("");
            let message = body
                .get("message")
                .and_then(Value::as_str)
                .filter(|m| !m.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| code.to_string());
            return match status.as_u16() {
                403 => Err(QuadError::Forbidden(if message.is_empty() {
                    "this action is not permitted for your student account".into()
                } else {
                    message
                })),
                404 => Err(QuadError::NotFound(path.to_string())),
                _ if code.contains("PRIVILEGE") || code.contains("NOT_ALLOWED") => {
                    Err(QuadError::Forbidden(message))
                }
                other => Err(QuadError::Http {
                    status: other,
                    body: text,
                }),
            };
        }
        if is_html {
            return Err(QuadError::SpaLoop(path.to_string()));
        }
        if text.trim().is_empty() {
            return Ok(json!({ "success": true }));
        }
        Ok(serde_json::from_str(&text).unwrap_or_else(|_| json!({ "status": "ok", "raw": text })))
    }

    // ------------------------------------------------------------------ //
    // Route wrappers
    // ------------------------------------------------------------------ //

    pub async fn me(&self) -> Result<Value, QuadError> {
        let v = self.get("/rest/service/authenticationDetails", &[]).await?;
        Ok(v.get("res")
            .and_then(|r| r.get("user"))
            .cloned()
            .unwrap_or(Value::Null))
    }

    pub async fn classes(&self) -> Result<Vec<Value>, QuadError> {
        let v = self.get("/rest/classes/v1/classroom", &[]).await?;
        Ok(as_array(v))
    }

    pub async fn terms(&self, programme: i64, batch: i64) -> Result<Vec<Value>, QuadError> {
        let v = self
            .get(
                "/api/terms/programmeBatchTerms",
                &[
                    ("programme", QueryArg::Int(programme)),
                    ("batch", QueryArg::Int(batch)),
                ],
            )
            .await?;
        Ok(as_array(v.get("terms").cloned().unwrap_or(Value::Null)))
    }

    pub async fn attendance(&self, ukid: i64, term_id: i64) -> Result<Value, QuadError> {
        self.get(
            &format!("/api/attendance/student/{ukid}/term/{term_id}"),
            &[],
        )
        .await
    }

    pub async fn assignments_page(
        &self,
        class_id: i64,
        ukid: i64,
        page: u32,
        per_page: u32,
    ) -> Result<Value, QuadError> {
        self.get(
            &format!("/rest/classes/{class_id}/users/{ukid}/assignments"),
            &[
                ("page", QueryArg::Int(page as i64)),
                ("perPage", QueryArg::Int(per_page as i64)),
            ],
        )
        .await
    }

    pub async fn assignment(&self, assignment_id: i64, class_id: i64) -> Result<Value, QuadError> {
        self.get(
            "/rest/assignments/v1/getStudentAssignmentDetails",
            &[
                ("assignmentId", QueryArg::Int(assignment_id)),
                ("classId", QueryArg::Int(class_id)),
            ],
        )
        .await
    }

    #[allow(dead_code)] // kept for parity with the portal's read routes
    pub async fn modules(&self, class_id: i64) -> Result<Vec<Value>, QuadError> {
        let v = self
            .get(&format!("/rest/classroomV2/class/{class_id}/module"), &[])
            .await?;
        Ok(as_array(v))
    }

    #[allow(dead_code)] // kept for parity with the portal's read routes
    pub async fn resources(&self, class_id: i64) -> Result<Vec<Value>, QuadError> {
        let v = self
            .get(
                "/rest/classroomV2/resources",
                &[("classId", QueryArg::Int(class_id))],
            )
            .await?;
        Ok(as_array(v))
    }

    pub async fn gradebook(&self, class_id: i64) -> Result<Value, QuadError> {
        self.get(
            "/rest/gradebook/v2/assessmentResult",
            &[("classId", QueryArg::Int(class_id))],
        )
        .await
    }

    pub async fn quizzes_page(
        &self,
        class_id: i64,
        page: u32,
        per_page: u32,
        status: Option<&str>,
        search: Option<&str>,
    ) -> Result<Value, QuadError> {
        let mut query = vec![
            ("page", QueryArg::Int(page as i64)),
            ("perPage", QueryArg::Int(per_page as i64)),
        ];
        if let Some(s) = status {
            query.push(("status", QueryArg::Str(s.to_string())));
        }
        if let Some(s) = search.filter(|s| !s.trim().is_empty()) {
            query.push(("search", QueryArg::Str(s.to_string())));
        }
        self.get(
            &format!("/api/classroomV2/quizQuestionnaire/student/{class_id}/list"),
            &query,
        )
        .await
    }

    #[allow(dead_code)] // kept for parity with the portal's read routes
    pub async fn discussions_page(
        &self,
        class_id: i64,
        page: u32,
        per_page: u32,
    ) -> Result<Value, QuadError> {
        self.get(
            "/rest/classroomDiscussions",
            &[
                ("classId", QueryArg::Int(class_id)),
                ("page", QueryArg::Int(page as i64)),
                ("perPage", QueryArg::Int(per_page as i64)),
            ],
        )
        .await
    }

    #[allow(dead_code)] // kept for parity with the portal's read routes
    pub async fn discussion(&self, id: i64) -> Result<Value, QuadError> {
        self.get(&format!("/rest/classroomDiscussions/discussion/{id}"), &[])
            .await
    }

    #[allow(dead_code)] // kept for parity with the portal's read routes
    pub async fn college_config(&self) -> Result<Value, QuadError> {
        let host = self
            .cfg
            .base_url
            .split("//")
            .nth(1)
            .unwrap_or("")
            .to_string();
        self.get(
            "/rest/service/collegeConfig",
            &[("url", QueryArg::Str(host))],
        )
        .await
    }

    pub async fn college_time(&self) -> Result<Value, QuadError> {
        self.get("/rest/structure/v1/collegeTime", &[]).await
    }

    pub async fn features(&self) -> Result<Vec<Value>, QuadError> {
        let v = self.get("/rest/service/features", &[]).await?;
        Ok(as_array(v.get("res").cloned().unwrap_or(v)))
    }

    /// Resolve a resource's fresh signed URL (signed URLs expire in ~1h).
    #[allow(dead_code)] // kept for parity with the portal's read routes
    pub async fn signed_url(&self, class_id: i64, media_id: i64) -> Result<Value, QuadError> {
        self.get(
            "/rest/classroomV2/resources/generateGetSignedUrlForPreview",
            &[
                ("classId", QueryArg::Int(class_id)),
                ("mediaId", QueryArg::Int(media_id)),
            ],
        )
        .await
    }

    // ------------------------------------------------------------------ //
    // Write and interactive questionnaire routes
    // ------------------------------------------------------------------ //

    pub async fn quiz_questions(&self, quiz_id: i64) -> Result<Value, QuadError> {
        self.get(
            &format!("/api/classroomV2/quizQuestionnaire/student/quiz/{quiz_id}/questions"),
            &[],
        )
        .await
    }

    pub async fn submit_quiz(&self, quiz_id: i64, body: &Value) -> Result<Value, QuadError> {
        self.post(
            &format!("/api/classroomV2/quizQuestionnaire/student/quiz/{quiz_id}/submit"),
            body,
        )
        .await
    }

    pub async fn submit_assignment(
        &self,
        class_id: i64,
        assignment_id: i64,
        body: &Value,
    ) -> Result<Value, QuadError> {
        let mut payload = body.clone();
        if let Some(map) = payload.as_object_mut() {
            map.insert("classId".into(), json!(class_id));
            map.insert("assignmentId".into(), json!(assignment_id));
        }
        self.post("/rest/assignments/v1/student/submit", &payload)
            .await
    }

    pub async fn feedback_forms(&self) -> Result<Vec<Value>, QuadError> {
        let v = self
            .get("/rest/feedback/v1/student/pendingForms", &[])
            .await?;
        Ok(as_array(v.get("forms").cloned().unwrap_or(v)))
    }

    pub async fn submit_feedback(&self, form_id: i64, body: &Value) -> Result<Value, QuadError> {
        self.post(
            &format!("/rest/feedback/v1/student/form/{form_id}/submit"),
            body,
        )
        .await
    }

    pub async fn submit_form(&self, path: &str, body: &Value) -> Result<Value, QuadError> {
        self.post(path, body).await
    }

    // ------------------------------------------------------------------ //
    // Derived reads
    // ------------------------------------------------------------------ //

    /// programmeId + batchYear, required by every term/attendance route.
    pub async fn profile_ids(&self) -> Result<(i64, i64), QuadError> {
        let me = self.me().await?;
        let programme = me.get("programmeId").and_then(Value::as_i64);
        let batch = me.get("batchYear").and_then(Value::as_i64);
        match (programme, batch) {
            (Some(p), Some(b)) => Ok((p, b)),
            _ => Err(QuadError::Parse(
                "the profile is missing programmeId/batchYear — attendance is unavailable for this account".into(),
            )),
        }
    }

    /// Every assignment for a class, merged across pages.
    pub async fn all_assignments(
        &self,
        class_id: i64,
        ukid: i64,
    ) -> Result<AssignmentBuckets, QuadError> {
        const PER_PAGE: u32 = 50;
        let mut buckets = AssignmentBuckets::default();
        let mut page = 1;
        loop {
            let data = self
                .assignments_page(class_id, ukid, page, PER_PAGE)
                .await?;
            let res = data.get("res").cloned().unwrap_or(Value::Null);
            if res.is_null() {
                break;
            }
            let upcoming = as_array(
                res.get("upcomingAssignments")
                    .cloned()
                    .unwrap_or(Value::Null),
            );
            buckets.upcoming.extend(upcoming.clone());
            buckets.previous.extend(as_array(
                res.get("previousAssignments")
                    .cloned()
                    .unwrap_or(Value::Null),
            ));
            buckets.draft.extend(as_array(
                res.get("draftAssignments").cloned().unwrap_or(Value::Null),
            ));
            let total = res
                .get("totalUpcomingCount")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            if (buckets.upcoming.len() as u64) >= total || upcoming.is_empty() || page >= 100 {
                break;
            }
            page += 1;
        }
        Ok(buckets)
    }
}

#[derive(Debug, Default)]
pub struct AssignmentBuckets {
    pub upcoming: Vec<Value>,
    pub previous: Vec<Value>,
    pub draft: Vec<Value>,
}

/// A query value; `None` values are dropped so absent options never hit the wire.
pub enum QueryArg {
    Int(i64),
    Str(String),
}

impl QueryArg {
    fn pair(&self, key: &str) -> Option<String> {
        match self {
            QueryArg::Int(n) => Some(format!("{key}={n}")),
            QueryArg::Str(s) => Some(format!("{key}={}", urlencode(s))),
        }
    }
}

/// Percent-encode everything outside the unreserved set.
pub fn urlencode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

pub fn as_array(v: Value) -> Vec<Value> {
    match v {
        Value::Array(items) => items,
        Value::Null => Vec::new(),
        other => {
            // `{ res: [...] }` style wrappers
            if let Some(inner) = other.get("res") {
                return as_array(inner.clone());
            }
            Vec::new()
        }
    }
}

/// Collapse the backend's non-breaking spaces and doubled whitespace.
pub fn clean(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or("")
        .replace('\u{a0}', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn int(v: &Value, key: &str) -> Value {
    v.get(key).cloned().unwrap_or(json!(null))
}

// ------------------------------------------------------------------------- //
// Dates
//
// Portal timestamps are naive college-local strings ("2026-11-10 23:00:00").
// Parsing them as UTC in a non-IST process silently shifts deadlines, so every
// timestamp the tools report is converted to a real instant in the college
// timezone and emitted as RFC 3339.
// ------------------------------------------------------------------------- //

/// Minutes east of UTC for `zone` at `at` (e.g. IST = 330).
pub fn zone_offset_minutes(zone: &str, at: i64) -> i64 {
    use chrono::{Offset, TimeZone};
    let Ok(tz) = zone.parse::<chrono_tz::Tz>() else {
        return 330;
    };
    let Some(dt) = chrono::Utc.timestamp_opt(at, 0).single() else {
        return 330;
    };
    (dt.with_timezone(&tz).offset().fix().local_minus_utc() / 60) as i64
}

/// Parse a portal timestamp → unix seconds. Naive values are read as
/// college-local wall clock time; already-offset values are used as-is.
pub fn naive_to_unix(value: &str, zone: &str) -> Option<i64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(value) {
        return Some(dt.timestamp());
    }
    naive_wall_to_unix(value, zone)
}

fn naive_wall_to_unix(value: &str, zone: &str) -> Option<i64> {
    use chrono::NaiveDateTime;
    let wall = ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M"]
        .iter()
        .find_map(|fmt| NaiveDateTime::parse_from_str(value, fmt).ok())
        .or_else(|| {
            chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .ok()
                .and_then(|d| d.and_hms_opt(0, 0, 0))
        })
        .map(|n| n.and_utc().timestamp())?;
    Some(wall - zone_offset_minutes(zone, wall) * 60)
}

/// RFC 3339 in the college timezone, or null for a missing/unreadable value.
pub fn rfc3339(value: &Value, zone: &str) -> Option<String> {
    let raw = value.as_str()?.trim();
    if raw.is_empty() {
        return None;
    }
    use chrono::{SecondsFormat, TimeZone};
    let unix = naive_to_unix(raw, zone)?;
    let tz = zone.parse::<chrono_tz::Tz>().ok()?;
    let dt = tz.timestamp_opt(unix, 0).single()?;
    Some(dt.to_rfc3339_opts(SecondsFormat::Secs, false))
}

/// Hours (negative = overdue) from now until the given naive portal datetime.
pub fn hours_until(value: &Value, zone: &str) -> Option<f64> {
    let raw = value.as_str()?;
    let unix = naive_to_unix(raw, zone)?;
    let now = crate::config::now_unix();
    Some(((unix - now) as f64 / 3600.0 * 10.0).round() / 10.0)
}
