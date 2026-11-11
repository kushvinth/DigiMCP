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
//!      in [`crate::error::DigiError::SpaLoop`], giving the calling AI agent an actionable
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
use crate::error::DigiError;

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
    pub fn new(cfg: Config) -> Result<Self, DigiError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "Accept",
            HeaderValue::from_static("application/json, text/plain, */*"),
        );
        headers.insert(
            "Auth-Token",
            HeaderValue::from_str(&cfg.token).map_err(|_| {
                DigiError::Config(
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
            .user_agent("digimcp (+https://github.com/kushvinth/DigiMCP)")
            .build()?;
        Ok(Self { http, cfg })
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    /// Execute an HTTP request and map portal errors onto actionable [`DigiError`] variants.
    async fn send_json(
        &self,
        req: reqwest::RequestBuilder,
        path: &str,
        strict_json: bool,
        forbidden_fallback: &str,
    ) -> Result<Value, DigiError> {
        let res = req.send().await.map_err(|e| {
            if e.status() == Some(reqwest::StatusCode::UNAUTHORIZED) {
                DigiError::Unauthorized
            } else {
                DigiError::from(e)
            }
        })?;

        let status = res.status();
        let text = res.text().await.map_err(DigiError::from)?;

        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(DigiError::Unauthorized);
        }
        let trimmed = text.trim_start();
        let is_html = trimmed.len() >= 9
            && (trimmed[..9].eq_ignore_ascii_case("<!doctype")
                || trimmed[..trimmed.len().min(5)].eq_ignore_ascii_case("<html"));
        if !status.is_success() {
            let body = serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
            let code = body.get("error").and_then(Value::as_str).unwrap_or("");
            let message = body
                .get("message")
                .and_then(Value::as_str)
                .filter(|m| !m.is_empty())
                .unwrap_or(code)
                .to_string();
            return match status.as_u16() {
                403 => Err(DigiError::Forbidden(if message.is_empty() {
                    forbidden_fallback.into()
                } else {
                    message
                })),
                404 => Err(DigiError::NotFound(path.to_string())),
                _ if code.contains("PRIVILEGE") || code.contains("NOT_ALLOWED") => {
                    Err(DigiError::Forbidden(message))
                }
                other => Err(DigiError::Http {
                    status: other,
                    body: text,
                }),
            };
        }
        if is_html {
            return Err(DigiError::SpaLoop(path.to_string()));
        }
        if text.trim().is_empty() {
            return Ok(if strict_json {
                Value::Null
            } else {
                json!({ "success": true })
            });
        }
        if strict_json {
            serde_json::from_str(&text).map_err(|e| DigiError::Parse(format!("{e} (in {path})")))
        } else {
            Ok(serde_json::from_str(&text)
                .unwrap_or_else(|_| json!({ "status": "ok", "raw": text })))
        }
    }

    /// GET a JSON route, mapping every failure onto an actionable error.
    pub async fn get(&self, path: &str, query: &[(&str, QueryArg)]) -> Result<Value, DigiError> {
        let mut url = format!("{}{}", self.cfg.base_url, path);
        let pairs: Vec<String> = query.iter().filter_map(|(k, v)| v.pair(k)).collect();
        if !pairs.is_empty() {
            url.push('?');
            url.push_str(&pairs.join("&"));
        }
        self.send_json(
            self.http.get(&url),
            path,
            true,
            "this route is not available to student accounts",
        )
        .await
    }

    /// POST a JSON route, mapping every failure onto an actionable error.
    pub async fn post(&self, path: &str, body: &Value) -> Result<Value, DigiError> {
        let url = format!("{}{}", self.cfg.base_url, path);
        self.send_json(
            self.http.post(&url).json(body),
            path,
            false,
            "this action is not permitted for your student account",
        )
        .await
    }

    /// PUT a route with optional query string and JSON body.
    pub async fn put_query(
        &self,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&Value>,
    ) -> Result<Value, DigiError> {
        let mut url = format!("{}{}", self.cfg.base_url, path);
        if !query.is_empty() {
            let qs: Vec<String> = query.iter().map(|(k, v)| format!("{k}={v}")).collect();
            url.push('?');
            url.push_str(&qs.join("&"));
        }
        let mut req = self.http.put(&url);
        if let Some(b) = body {
            req = req.json(b);
        }
        self.send_json(
            req,
            path,
            false,
            "this action is not permitted for your student account",
        )
        .await
    }

    // ------------------------------------------------------------------ //
    // Route wrappers
    // ------------------------------------------------------------------ //

    pub async fn me(&self) -> Result<Value, DigiError> {
        let v = self.get("/rest/service/authenticationDetails", &[]).await?;
        Ok(v.get("res")
            .and_then(|r| r.get("user"))
            .cloned()
            .unwrap_or(Value::Null))
    }

    pub async fn classes(&self) -> Result<Vec<Value>, DigiError> {
        let v = self.get("/rest/classes/v1/classroom", &[]).await?;
        Ok(as_array(v))
    }

    pub async fn terms(&self, programme: i64, batch: i64) -> Result<Vec<Value>, DigiError> {
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

    pub async fn attendance(&self, ukid: i64, term_id: i64) -> Result<Value, DigiError> {
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
    ) -> Result<Value, DigiError> {
        self.get(
            &format!("/rest/classes/{class_id}/users/{ukid}/assignments"),
            &[
                ("page", QueryArg::Int(page as i64)),
                ("perPage", QueryArg::Int(per_page as i64)),
            ],
        )
        .await
    }

    pub async fn assignment(&self, assignment_id: i64, class_id: i64) -> Result<Value, DigiError> {
        self.get(
            "/rest/assignments/v1/getStudentAssignmentDetails",
            &[
                ("assignmentId", QueryArg::Int(assignment_id)),
                ("classId", QueryArg::Int(class_id)),
            ],
        )
        .await
    }

    pub async fn modules(&self, class_id: i64) -> Result<Vec<Value>, DigiError> {
        let v = self
            .get(&format!("/rest/classroomV2/class/{class_id}/module"), &[])
            .await?;
        Ok(as_array(v))
    }

    pub async fn resources(&self, class_id: i64) -> Result<Vec<Value>, DigiError> {
        let v = self
            .get(
                "/rest/classroomV2/resources",
                &[("classId", QueryArg::Int(class_id))],
            )
            .await?;
        Ok(as_array(v))
    }

    pub async fn gradebook(&self, class_id: i64) -> Result<Value, DigiError> {
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
    ) -> Result<Value, DigiError> {
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

    pub async fn college_time(&self) -> Result<Value, DigiError> {
        self.get("/rest/structure/v1/collegeTime", &[]).await
    }

    pub async fn college_config(&self) -> Result<Value, DigiError> {
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

    pub async fn features(&self) -> Result<Vec<Value>, DigiError> {
        let v = self.get("/rest/service/features", &[]).await?;
        Ok(as_array(v.get("res").cloned().unwrap_or(v)))
    }

    /// Resolve a resource's fresh signed URL (signed URLs expire in ~1h).
    pub async fn signed_url(&self, class_id: i64, media_id: i64) -> Result<Value, DigiError> {
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

    pub async fn quiz_questions(&self, quiz_id: i64) -> Result<Value, DigiError> {
        self.get(
            &format!("/api/classroomV2/quizQuestionnaire/student/quiz/{quiz_id}/questions"),
            &[],
        )
        .await
    }

    pub async fn submit_quiz(&self, quiz_id: i64, body: &Value) -> Result<Value, DigiError> {
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
    ) -> Result<Value, DigiError> {
        let mut payload = body.clone();
        if let Some(map) = payload.as_object_mut() {
            map.insert("classId".into(), json!(class_id));
            map.insert("assignmentId".into(), json!(assignment_id));
        }
        self.post("/rest/assignments/v1/student/submit", &payload)
            .await
    }

    pub async fn feedback_forms(&self) -> Result<Vec<Value>, DigiError> {
        // First try the Digiicampus /api/feedback sessions route
        if let Ok(sessions) = self.get("/api/feedback/student/sessions", &[]).await {
            let sess_arr = as_array(sessions);
            if !sess_arr.is_empty() {
                let mut all_forms = Vec::new();
                for s in sess_arr {
                    let session_id = s.get("sessionId").and_then(Value::as_i64).unwrap_or(0);
                    let session_name = clean(s.get("sessionName"));
                    let end_date = s.get("endDate").cloned().unwrap_or(Value::Null);
                    let q = [("sessionId", QueryArg::Int(session_id))];
                    if let Ok(courses_val) = self.get("/api/feedback/student/courses", &q).await {
                        for c in as_array(courses_val) {
                            let course_name = clean(c.get("courseName"));
                            let course_code = clean(c.get("courseCode"));
                            for comp in
                                as_array(c.get("components").cloned().unwrap_or(Value::Null))
                            {
                                let comp_name = clean(comp.get("courseComponentTypeName"));
                                for fac in
                                    as_array(comp.get("faculties").cloned().unwrap_or(Value::Null))
                                {
                                    let fac_name = clean(fac.get("facultyName"));
                                    let cft_id = fac
                                        .get("courseFacultyTemplateId")
                                        .and_then(Value::as_i64)
                                        .unwrap_or(0);
                                    let submitted = fac
                                        .get("templateSubmitted")
                                        .and_then(Value::as_bool)
                                        .unwrap_or(false);
                                    all_forms.push(json!({
                                        "formId": cft_id,
                                        "title": format!("{course_name} - {fac_name}"),
                                        "courseName": course_name,
                                        "courseCode": course_code,
                                        "component": comp_name,
                                        "facultyName": fac_name,
                                        "sessionId": session_id,
                                        "sessionName": session_name,
                                        "status": if submitted { "SUBMITTED" } else { "PENDING" },
                                        "deadline": end_date,
                                    }));
                                }
                            }
                        }
                    }
                }
                if !all_forms.is_empty() {
                    return Ok(all_forms);
                }
            }
        }

        // Fallback to older /rest/feedback/v1/student/pendingForms route
        let v = self
            .get("/rest/feedback/v1/student/pendingForms", &[])
            .await?;
        Ok(as_array(v.get("forms").cloned().unwrap_or(v)))
    }

    /// Fetch dynamic feedback questionnaire template to inspect sections, questions, and required coverage.
    pub async fn feedback_template(
        &self,
        session_id: i64,
        cft_id: i64,
    ) -> Result<Value, DigiError> {
        let q_tmpl = [
            ("sessionId", QueryArg::Int(session_id)),
            ("courseFacultyTemplateId", QueryArg::Int(cft_id)),
        ];
        self.get("/api/feedback/student/template", &q_tmpl).await
    }

    pub async fn submit_feedback_dynamic(
        &self,
        session_id: i64,
        cft_id: i64,
        ratings: &[Value],
    ) -> Result<Value, DigiError> {
        let mut tmpl = self.feedback_template(session_id, cft_id).await?;
        let status_id = tmpl
            .get("studentTemplateStatusId")
            .and_then(Value::as_i64)
            .unwrap_or(0);

        if let Some(sections) = tmpl.get_mut("sections").and_then(Value::as_array_mut) {
            for sec in sections {
                if let Some(questions) = sec.get_mut("questions").and_then(Value::as_array_mut) {
                    for q in questions {
                        let q_id = q.get("questionId").and_then(Value::as_i64).unwrap_or(0);
                        let q_type = clean(q.get("type"));
                        let is_optional = q
                            .get("optionalQuestion")
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                        let q_text = clean(q.get("questionText"));

                        let rating_match = ratings
                            .iter()
                            .find(|r| r.get("questionId").and_then(Value::as_i64) == Some(q_id));

                        if q_type == "SINGLE_SELECT" {
                            let target_score = if let Some(rm) = rating_match {
                                rm.get("rating").and_then(Value::as_f64)
                            } else {
                                None
                            };

                            let score = match target_score {
                                Some(s) => s,
                                None => {
                                    if is_optional {
                                        continue;
                                    }
                                    return Err(DigiError::Validation(format!(
                                        "Missing rating for question {q_id}: \"{q_text}\". You must explicitly provide a rating score (1-5)."
                                    )));
                                }
                            };

                            if let Some(opts) = q.get_mut("options").and_then(Value::as_array_mut) {
                                for opt in opts {
                                    let opt_score = opt
                                        .get("optionScore")
                                        .and_then(Value::as_f64)
                                        .unwrap_or(0.0);
                                    let is_match = (opt_score - score).abs() < 0.1;
                                    opt["optionSelected"] = json!(is_match);
                                }
                            }
                        } else if q_type == "SUBJECTIVE" {
                            let text = rating_match
                                .and_then(|r| r.get("remarks").and_then(Value::as_str))
                                .unwrap_or("");
                            if text.trim().is_empty() && !is_optional {
                                return Err(DigiError::Validation(format!(
                                    "Missing remarks for question {q_id}: \"{q_text}\". You must provide feedback text."
                                )));
                            }
                            q["responseText"] = json!(text);
                        }
                    }
                }
            }
        }

        let id_str = status_id.to_string();
        self.put_query(
            "/api/feedback/student/template/response",
            &[("studentTemplateStatusId", &id_str)],
            Some(&tmpl),
        )
        .await?;

        self.put_query(
            "/api/feedback/student/template/submit",
            &[("studentTemplateStatusId", &id_str)],
            None,
        )
        .await
    }

    pub async fn submit_feedback(&self, form_id: i64, body: &Value) -> Result<Value, DigiError> {
        self.post(
            &format!("/rest/feedback/v1/student/form/{form_id}/submit"),
            body,
        )
        .await
    }

    pub async fn submit_form(&self, path: &str, body: &Value) -> Result<Value, DigiError> {
        self.post(path, body).await
    }

    // ------------------------------------------------------------------ //
    // Derived reads
    // ------------------------------------------------------------------ //

    /// programmeId + batchYear, required by every term/attendance route.
    pub async fn profile_ids(&self) -> Result<(i64, i64), DigiError> {
        let me = self.me().await?;
        let programme = me.get("programmeId").and_then(Value::as_i64);
        let batch = me.get("batchYear").and_then(Value::as_i64);
        match (programme, batch) {
            (Some(p), Some(b)) => Ok((p, b)),
            _ => Err(DigiError::Parse(
                "the profile is missing programmeId/batchYear — attendance is unavailable for this account".into(),
            )),
        }
    }

    /// Every assignment for a class, merged across pages.
    pub async fn all_assignments(
        &self,
        class_id: i64,
        ukid: i64,
    ) -> Result<AssignmentBuckets, DigiError> {
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

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(y) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Howard Hinnant's civil-to-days algorithm (epoch = 1970-01-01).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let m = m as i64;
    let d = d as i64;
    let y = y - if m <= 2 { 1 } else { 0 };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Howard Hinnant's days-to-civil algorithm (epoch = 1970-01-01).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (y + if m <= 2 { 1 } else { 0 }, m as u32, d as u32)
}

fn parse_two(b: &[u8]) -> Option<u32> {
    if b.len() < 2 || !b[0].is_ascii_digit() || !b[1].is_ascii_digit() {
        return None;
    }
    Some(((b[0] - b'0') * 10 + (b[1] - b'0')) as u32)
}

fn parse_tz_offset_suffix(s: &str) -> Option<i64> {
    if s.eq_ignore_ascii_case("z") {
        return Some(0);
    }
    let b = s.as_bytes();
    let sign = match *b.first()? {
        b'+' => 1i64,
        b'-' => -1i64,
        _ => return None,
    };
    let rest = &b[1..];
    let (hh, mm) = if rest.len() == 5 && rest[2] == b':' {
        (parse_two(&rest[0..2])?, parse_two(&rest[3..5])?)
    } else if rest.len() == 4 {
        (parse_two(&rest[0..2])?, parse_two(&rest[2..4])?)
    } else if rest.len() == 2 {
        (parse_two(&rest[0..2])?, 0)
    } else {
        return None;
    };
    if hh > 23 || mm > 59 {
        return None;
    }
    Some(sign * (hh as i64 * 60 + mm as i64) * 60)
}

fn parse_wall_components(s: &str) -> Option<(i64, Option<i64>)> {
    let b = s.as_bytes();
    if b.len() < 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let y: i64 = std::str::from_utf8(&b[0..4]).ok()?.parse().ok()?;
    let m = parse_two(&b[5..7])?;
    let d = parse_two(&b[8..10])?;
    if !(1..=12).contains(&m) || d == 0 || d > days_in_month(y, m) {
        return None;
    }

    let mut pos = 10usize;
    let (hh, mm, ss) = if pos < b.len() && (b[pos] == b' ' || b[pos] == b'T' || b[pos] == b't') {
        pos += 1;
        if b.len() < pos + 5 || b[pos + 2] != b':' {
            return None;
        }
        let hh = parse_two(&b[pos..pos + 2])?;
        let mm = parse_two(&b[pos + 3..pos + 5])?;
        pos += 5;
        let ss = if pos < b.len() && b[pos] == b':' {
            pos += 1;
            let ss = parse_two(b[pos..].get(..2)?)?;
            pos += 2;
            ss
        } else {
            0
        };
        if pos < b.len() && b[pos] == b'.' {
            pos += 1;
            while pos < b.len() && b[pos].is_ascii_digit() {
                pos += 1;
            }
        }
        if hh > 23 || mm > 59 || ss > 59 {
            return None;
        }
        (hh as i64, mm as i64, ss as i64)
    } else {
        (0, 0, 0)
    };

    let wall_unix = days_from_civil(y, m, d) * 86_400 + hh * 3600 + mm * 60 + ss;
    let tail = std::str::from_utf8(&b[pos..]).ok()?.trim();
    if tail.is_empty() {
        Some((wall_unix, None))
    } else {
        Some((wall_unix, Some(parse_tz_offset_suffix(tail)?)))
    }
}

fn read_be_u32(b: &[u8]) -> Option<usize> {
    Some(u32::from_be_bytes(b.try_into().ok()?) as usize)
}

fn tzif_offset_minutes(bytes: &[u8], at: i64) -> Option<i64> {
    if bytes.len() < 44 || &bytes[0..4] != b"TZif" {
        return None;
    }
    let version = bytes[4];
    let mut off = 20usize;
    let isut = read_be_u32(bytes.get(off..off + 4)?)?;
    let isstd = read_be_u32(bytes.get(off + 4..off + 8)?)?;
    let leap = read_be_u32(bytes.get(off + 8..off + 12)?)?;
    let timecnt = read_be_u32(bytes.get(off + 12..off + 16)?)?;
    let typecnt = read_be_u32(bytes.get(off + 16..off + 20)?)?;
    let charcnt = read_be_u32(bytes.get(off + 20..off + 24)?)?;

    let is_v2 = version >= b'2';
    let (time_sz, leap_sz, base_off) = if is_v2 {
        let v1_len = 44 + timecnt * 5 + typecnt * 6 + charcnt + leap * 8 + isstd + isut;
        if bytes.get(v1_len..v1_len + 4) != Some(b"TZif") {
            return None;
        }
        (8usize, 12usize, v1_len)
    } else {
        (4usize, 8usize, 0usize)
    };

    let h = base_off + 20;
    let _isut2 = read_be_u32(bytes.get(h..h + 4)?)?;
    let _isstd2 = read_be_u32(bytes.get(h + 4..h + 8)?)?;
    let _leap2 = read_be_u32(bytes.get(h + 8..h + 12)?)?;
    let timecnt2 = read_be_u32(bytes.get(h + 12..h + 16)?)?;
    let typecnt2 = read_be_u32(bytes.get(h + 16..h + 20)?)?;
    if typecnt2 == 0 {
        return None;
    }
    let _ = _leap2 * leap_sz;
    off = base_off + 44;

    let trans_bytes = timecnt2.checked_mul(time_sz)?;
    let trans_slice = bytes.get(off..off + trans_bytes)?;
    off += trans_bytes;
    let idx_slice = bytes.get(off..off + timecnt2)?;
    off += timecnt2;
    let ttinfo_slice = bytes.get(off..off + typecnt2 * 6)?;

    let mut chosen_idx = 0usize;
    if timecnt2 > 0 {
        let trans_at = |i: usize| -> Option<i64> {
            let s = &trans_slice[i * time_sz..(i + 1) * time_sz];
            if time_sz == 8 {
                Some(i64::from_be_bytes(s.try_into().ok()?))
            } else {
                Some(i32::from_be_bytes(s.try_into().ok()?) as i64)
            }
        };
        let mut lo = 0usize;
        let mut hi = timecnt2;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if trans_at(mid)? <= at {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo > 0 {
            chosen_idx = idx_slice[lo - 1] as usize;
        } else {
            for t in 0..typecnt2 {
                if ttinfo_slice[t * 6 + 4] == 0 {
                    chosen_idx = t;
                    break;
                }
            }
        }
    }
    if chosen_idx >= typecnt2 {
        return None;
    }
    let utoff = i32::from_be_bytes(
        ttinfo_slice[chosen_idx * 6..chosen_idx * 6 + 4]
            .try_into()
            .ok()?,
    );
    Some((utoff / 60) as i64)
}

/// Minutes east of UTC for `zone` at `at` (e.g. IST = 330).
pub fn zone_offset_minutes(zone: &str, at: i64) -> i64 {
    let z = zone.trim();
    match z {
        "Asia/Kolkata" | "Asia/Calcutta" | "IST" => return 330,
        "UTC" | "Etc/UTC" | "GMT" | "Z" => return 0,
        _ => {}
    }
    if let Some(secs) = parse_tz_offset_suffix(z) {
        return secs / 60;
    }
    if z.is_empty() || z.starts_with('/') || z.contains("..") || z.contains('\\') {
        return 330;
    }
    for root in ["/usr/share/zoneinfo", "/var/db/timezone/zoneinfo"] {
        let path = std::path::Path::new(root).join(z);
        if let Ok(bytes) = std::fs::read(&path)
            && let Some(mins) = tzif_offset_minutes(&bytes, at)
        {
            return mins;
        }
    }
    330
}

fn civil_parts(unix: i64) -> (i64, u32, u32, i64, i64, i64) {
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    (y, m, d, secs / 3600, (secs % 3600) / 60, secs % 60)
}

fn format_rfc3339_offset(unix: i64, offset_minutes: i64) -> String {
    let (y, m, d, hh, mm, ss) = civil_parts(unix + offset_minutes * 60);
    let sign = if offset_minutes >= 0 { '+' } else { '-' };
    let abs = offset_minutes.abs();
    format!(
        "{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}{sign}{:02}:{:02}",
        abs / 60,
        abs % 60
    )
}

/// Format a unix timestamp (seconds) as UTC RFC 3339 (`YYYY-MM-DDTHH:MM:SSZ`).
pub fn unix_to_rfc3339(ts: i64) -> Option<String> {
    let (y, m, d, hh, mm, ss) = civil_parts(ts);
    Some(format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z"))
}

/// Parse a portal timestamp → unix seconds. Naive values are read as
/// college-local wall clock time; already-offset values are used as-is.
pub fn naive_to_unix(value: &str, zone: &str) -> Option<i64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let (wall, explicit_offset) = parse_wall_components(value)?;
    match explicit_offset {
        Some(secs) => Some(wall - secs),
        None => {
            let approx = wall - zone_offset_minutes(zone, wall) * 60;
            Some(wall - zone_offset_minutes(zone, approx) * 60)
        }
    }
}

/// RFC 3339 in the college timezone, or null for a missing/unreadable value.
pub fn rfc3339(value: &Value, zone: &str) -> Option<String> {
    let raw = value.as_str()?.trim();
    if raw.is_empty() {
        return None;
    }
    let unix = naive_to_unix(raw, zone)?;
    let offset_min = zone_offset_minutes(zone, unix);
    Some(format_rfc3339_offset(unix, offset_min))
}

/// Hours (negative = overdue) from now until the given naive portal datetime.
pub fn hours_until(value: &Value, zone: &str) -> Option<f64> {
    let raw = value.as_str()?;
    let unix = naive_to_unix(raw, zone)?;
    let now = crate::config::now_unix();
    Some(((unix - now) as f64 / 3600.0 * 10.0).round() / 10.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rfc3339_ist_and_explicit_offset() {
        assert_eq!(
            rfc3339(&json!("2026-05-10 14:30:00"), "Asia/Kolkata").as_deref(),
            Some("2026-05-10T14:30:00+05:30")
        );
        assert_eq!(
            rfc3339(&json!("2026-05-10T09:00:00Z"), "Asia/Kolkata").as_deref(),
            Some("2026-05-10T14:30:00+05:30")
        );
    }

    #[test]
    fn rfc3339_dst_transitions_and_leap_day() {
        // America/New_York switches between EST (-05:00) and EDT (-04:00).
        if std::path::Path::new("/usr/share/zoneinfo/America/New_York").exists()
            || std::path::Path::new("/var/db/timezone/zoneinfo/America/New_York").exists()
        {
            assert_eq!(
                rfc3339(&json!("2026-01-15 12:00:00"), "America/New_York").as_deref(),
                Some("2026-01-15T12:00:00-05:00")
            );
            assert_eq!(
                rfc3339(&json!("2026-07-15 12:00:00"), "America/New_York").as_deref(),
                Some("2026-07-15T12:00:00-04:00")
            );
        }
        assert_eq!(
            rfc3339(&json!("2024-02-29 23:59:59"), "UTC").as_deref(),
            Some("2024-02-29T23:59:59+00:00")
        );
        assert_eq!(
            unix_to_rfc3339(1_709_251_199).as_deref(),
            Some("2024-02-29T23:59:59Z")
        );
    }
}
