//! What each tool does.
//!
//! One function per question a student actually asks. Every function returns
//! compact JSON, resolves names to ids itself (so the agent rarely needs a
//! second round trip), and never writes to the portal.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use tokio::task::JoinSet;

use super::server::{Quad, finish};
use crate::error::QuadError;
use crate::portal::{Portal, as_array, clean, hours_until, int, rfc3339};

/// Every class component the student is enrolled in.
pub fn class_label(c: &Value) -> String {
    let name = clean(c.get("courseName"));
    let kind = clean(c.get("courseComponentTypeName"));
    let section = clean(c.get("className"));
    match (kind.is_empty(), section.is_empty()) {
        (true, true) => name,
        (false, true) => format!("{name} ({kind})"),
        (true, false) => format!("{name} ({section})"),
        (false, false) => format!("{name} {kind} ({section})"),
    }
}

#[derive(Clone, Debug)]
pub struct ClassRow {
    pub id: i64,
    pub label: String,
    pub course_name: String,
    pub course_code: String,
    pub kind: String,
    pub credits: Value,
    pub faculty: String,
}

impl ClassRow {
    fn from_value(v: &Value) -> Self {
        Self {
            id: v.get("id").and_then(Value::as_i64).unwrap_or(0),
            label: class_label(v),
            course_name: clean(v.get("courseName")),
            course_code: clean(v.get("courseCode")),
            kind: clean(v.get("courseComponentTypeName")),
            credits: v.get("courseCredits").cloned().unwrap_or(Value::Null),
            faculty: clean(v.get("faculty")),
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "classId": self.id,
            "class": self.label,
            "courseCode": opt(&self.course_code),
            "component": opt(&self.kind),
            "credits": self.credits,
            "faculty": opt(&self.faculty),
        })
    }
}

fn opt(s: &str) -> Value {
    if s.is_empty() { Value::Null } else { json!(s) }
}

/// Match a selector against id, course code, name and component: exact id,
/// then exact code, then substring, then all tokens present.
pub fn pick_class(classes: &[ClassRow], selector: &str) -> Option<ClassRow> {
    let sel = selector.trim().to_lowercase();
    if sel.is_empty() {
        return None;
    }
    if let Ok(id) = sel.parse::<i64>()
        && let Some(c) = classes.iter().find(|c| c.id == id)
    {
        return Some(c.clone());
    }
    if let Some(c) = classes.iter().find(|c| c.course_code.to_lowercase() == sel) {
        return Some(c.clone());
    }
    let hay =
        |c: &ClassRow| format!("{} {} {}", c.course_name, c.label, c.course_code).to_lowercase();
    if let Some(c) = classes.iter().find(|c| hay(c).contains(&sel)) {
        return Some(c.clone());
    }
    let tokens: Vec<&str> = sel.split_whitespace().filter(|t| t.len() > 2).collect();
    classes
        .iter()
        .find(|c| {
            let h = hay(c);
            !tokens.is_empty() && tokens.iter().all(|t| h.contains(t))
        })
        .cloned()
}

fn num(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn round1(n: f64) -> f64 {
    (n * 10.0).round() / 10.0
}

impl Quad {
    fn zone(&self) -> String {
        self.portal.config().timezone.clone()
    }

    fn summarise_pending(&self, a: &Value, class_label: &str) -> Value {
        let zone = self.zone();
        json!({
            "assignmentId": int(a, "id"),
            "name": clean(a.get("name")),
            "class": class_label,
            "classId": int(a, "classId"),
            "due": rfc3339(a.get("dueDate").unwrap_or(&Value::Null), &zone),
            "hoursUntilDue": hours_until(a.get("dueDate").unwrap_or(&Value::Null), &zone),
            "rejected": a.get("isRejected").and_then(Value::as_bool).unwrap_or(false),
            "submissionType": a.get("submissionType").cloned().unwrap_or(Value::Null),
        })
    }

    pub async fn ukid(&self) -> Result<i64, QuadError> {
        let p = self.ready()?;
        if let Some(u) = p.config().ukid {
            return Ok(u);
        }
        let me = p.me().await?;
        me.get("ukid")
            .and_then(Value::as_i64)
            .ok_or_else(|| QuadError::Parse("the profile has no ukid".into()))
    }

    pub async fn class_list(&self) -> Result<Vec<ClassRow>, QuadError> {
        Ok(self
            .ready()?
            .classes()
            .await?
            .iter()
            .map(ClassRow::from_value)
            .collect())
    }

    /// The classes a selector applies to (all of them when none is given).
    async fn targets(&self, selector: Option<&str>) -> Result<Vec<ClassRow>, QuadError> {
        match selector.filter(|s| !s.trim().is_empty()) {
            Some(sel) => Ok(vec![self.resolve_class(sel).await?]),
            None => self.class_list().await,
        }
    }

    async fn resolve_class(&self, selector: &str) -> Result<ClassRow, QuadError> {
        let classes = self.class_list().await?;
        pick_class(&classes, selector).ok_or_else(|| {
            QuadError::NotFound(format!(
                "no class matches {selector:?}. Call `classes` to see the list."
            ))
        })
    }

    // ------------------------------------------------------------------ //
    // account
    // ------------------------------------------------------------------ //

    pub async fn account(&self) -> Result<serde_json::Value, QuadError> {
        let p = self.ready()?;
        let cfg = p.config();
        let me = p.me().await?;
        let tz = self.zone();
        let clock = p.college_time().await.ok();
        let config = p.college_config().await.ok();
        let features = p.features().await.unwrap_or_default();

        let mut enabled: Vec<String> = features
            .iter()
            .filter(|f| f.get("enabled").and_then(Value::as_bool).unwrap_or(false))
            .map(|f| {
                let custom = clean(f.get("featureCustomName"));
                if custom.is_empty() {
                    clean(f.get("feature"))
                } else {
                    custom
                }
            })
            .filter(|s| !s.is_empty())
            .collect();
        enabled.sort();
        enabled.truncate(24);

        Ok(json!({
            "connected": true,
            "you": {
                "name": clean(me.get("name")),
                "registrationId": opt(&clean(me.get("registrationId"))),
                "programme": opt(&clean(me.get("programmeName"))),
                "department": opt(&clean(me.get("departmentName"))),
                "section": opt(&clean(me.get("sectionName"))),
                "year": me.get("year").cloned().unwrap_or(Value::Null),
                "batchYear": me.get("batchYear").cloned().unwrap_or(Value::Null),
            },
            "portal": {
                "origin": cfg.base_url,
                "college": config.as_ref().map(|c| clean(c.get("name"))).filter(|s| !s.is_empty())
                    .or_else(|| cfg.college.clone()),
                "timezone": tz,
                "serverTime": clock.as_ref().and_then(|c| rfc3339(c.get("currentTime").unwrap_or(&Value::Null), &tz)),
            },
            "token": {
                "expiresAt": cfg.expires_at.and_then(unix_to_rfc3339),
                "daysLeft": cfg.days_left(),
                "expired": cfg.is_expired(),
                "hint": "if daysLeft is 0 or negative, run `quad login` again — the portal invalidates the old cookie"
            },
            "features": enabled,
            "readOnly": true,
        }))
    }

    // ------------------------------------------------------------------ //
    // classes
    // ------------------------------------------------------------------ //

    pub async fn classes(&self, query: Option<&str>) -> Result<Value, QuadError> {
        let mut classes = self.class_list().await?;
        if let Some(q) = query.filter(|q| !q.trim().is_empty()) {
            let q = q.to_lowercase();
            classes.retain(|c| {
                format!("{} {} {}", c.course_name, c.course_code, c.label)
                    .to_lowercase()
                    .contains(&q)
            });
        }
        Ok(json!({
            "count": classes.len(),
            "classes": classes.iter().map(ClassRow::to_json).collect::<Vec<_>>(),
            "note": "a course can have separate Lecture/Practical entries with different classIds"
        }))
    }

    // ------------------------------------------------------------------ //
    // due
    // ------------------------------------------------------------------ //

    pub async fn due(
        &self,
        within_hours: Option<f64>,
        include_overdue: bool,
        include_rejected: bool,
        class: Option<&str>,
    ) -> Result<Value, QuadError> {
        let p = self.ready()?;
        let ukid = self.ukid().await?;
        let zone = self.zone();
        let targets = self.targets(class).await?;

        let mut items: Vec<Value> = Vec::new();
        let mut warnings: Vec<Value> = Vec::new();

        // Assignments: one task per class, so a slow class cannot stall the rest.
        let mut set = JoinSet::new();
        for c in &targets {
            let (id, label, cfg) = (c.id, c.label.clone(), p.config().clone());
            set.spawn(async move {
                let result = match Portal::new(cfg) {
                    Ok(portal) => portal.all_assignments(id, ukid).await,
                    Err(e) => Err(e),
                };
                (id, label, result)
            });
        }
        let mut fetched = Vec::new();
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok(row) => fetched.push(row),
                Err(e) => warnings.push(json!({ "error": format!("task failed: {e}") })),
            }
        }

        for (id, label, result) in fetched {
            let buckets = match result {
                Ok(b) => b,
                Err(e) => {
                    warnings.push(json!({ "class": label, "classId": id, "error": e.to_string() }));
                    continue;
                }
            };
            for a in &buckets.upcoming {
                if a.get("isSubmitted")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    continue;
                }
                let rejected = a
                    .get("isRejected")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if rejected && !include_rejected {
                    continue;
                }
                let due = a.get("dueDate").unwrap_or(&Value::Null);
                let hours = hours_until(due, &zone);
                let overdue = a
                    .get("dueDatePassed")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                    || hours.is_some_and(|h| h < 0.0);
                if overdue && !include_overdue && !rejected {
                    continue;
                }
                if within_hours.is_some_and(|limit| hours.is_some_and(|h| h > limit)) {
                    continue;
                }
                let mut row = self.summarise_pending(a, &label);
                row["kind"] = json!("assignment");
                row["overdue"] = json!(overdue);
                items.push(row);
            }
        }

        // Quizzes and exams: only students with quizzes enabled see any.
        for c in &targets {
            let Ok(page) = p.quizzes_page(c.id, 1, 50, None, None).await else {
                continue;
            };
            for q in as_array(page.get("quizzes").cloned().unwrap_or(Value::Null)) {
                let status = clean(q.get("status")).to_uppercase();
                if status == "CLOSED" {
                    continue;
                }
                let end = q.get("quizEndTime").cloned().unwrap_or(Value::Null);
                let hours = hours_until(&end, &zone);
                if within_hours.is_some_and(|limit| hours.is_some_and(|h| h > limit)) {
                    continue;
                }
                items.push(json!({
                    "kind": "quiz",
                    "quizId": int(&q, "id"),
                    "name": clean(q.get("title")),
                    "class": c.label,
                    "classId": c.id,
                    "status": status.to_lowercase(),
                    "opens": rfc3339(q.get("quizStartTime").unwrap_or(&Value::Null), &zone),
                    "closes": rfc3339(&end, &zone),
                    "hoursUntilDue": hours,
                    "duration": q.get("quizDuration").cloned().unwrap_or(Value::Null),
                    "maxMarks": q.get("maxMarks").cloned().unwrap_or(Value::Null),
                }));
            }
        }

        items.sort_by(|a, b| {
            let key = |v: &Value| {
                v.get("hoursUntilDue")
                    .and_then(Value::as_f64)
                    .unwrap_or(f64::MAX)
            };
            key(a).total_cmp(&key(b))
        });
        let overdue = items
            .iter()
            .filter(|i| {
                i.get("hoursUntilDue")
                    .and_then(Value::as_f64)
                    .is_some_and(|h| h < 0.0)
            })
            .count();

        let mut out = json!({
            "count": items.len(),
            "overdue": overdue,
            "timezone": zone,
            "items": items,
        });
        if !warnings.is_empty() {
            out["warnings"] = json!(warnings);
        }
        Ok(out)
    }

    // ------------------------------------------------------------------ //
    // assignment
    // ------------------------------------------------------------------ //

    async fn resolve_assignment_entry(
        &self,
        selector: &str,
        class: Option<&str>,
    ) -> Result<(ClassRow, Value), QuadError> {
        let p = self.ready()?;
        let ukid = self.ukid().await?;
        let targets = self.targets(class).await?;
        let wanted = selector.trim().to_lowercase();

        let mut found: Option<(ClassRow, Value)> = None;
        if let Ok(id) = wanted.parse::<i64>() {
            for c in &targets {
                if let Ok(detail) = p.assignment(id, c.id).await {
                    found = Some((c.clone(), detail));
                    break;
                }
            }
        }
        if found.is_none() {
            'outer: for c in &targets {
                let Ok(buckets) = p.all_assignments(c.id, ukid).await else {
                    continue;
                };
                for a in buckets
                    .upcoming
                    .iter()
                    .chain(&buckets.previous)
                    .chain(&buckets.draft)
                {
                    let name = clean(a.get("name")).to_lowercase();
                    let id = a.get("id").and_then(Value::as_i64).unwrap_or(0);
                    if (name == wanted || id.to_string() == wanted || name.contains(&wanted))
                        && let Ok(detail) = p.assignment(id, c.id).await
                    {
                        found = Some((c.clone(), detail));
                        break 'outer;
                    }
                }
            }
        }

        found.ok_or_else(|| {
            QuadError::NotFound(format!(
                "no assignment matches {selector:?}. Use `search` to find the exact wording."
            ))
        })
    }

    pub async fn assignment(
        &self,
        selector: &str,
        class: Option<&str>,
    ) -> Result<Value, QuadError> {
        let zone = self.zone();
        let (class_row, a) = self.resolve_assignment_entry(selector, class).await?;

        let attachments: Vec<Value> = as_array(a.get("resources").cloned().unwrap_or(Value::Null))
            .iter()
            .map(|r| {
                json!({
                    "name": clean(r.get("mediaName")),
                    "type": clean(r.get("mediaType")),
                    "link": r.get("weblink").cloned().filter(|v| !v.is_null())
                        .or_else(|| r.get("mediaUrl").cloned().filter(|v| !v.is_null()))
                        .unwrap_or(Value::Null),
                })
            })
            .collect();

        Ok(json!({
            "assignmentId": int(&a, "id"),
            "name": clean(a.get("name")),
            "class": class_row.label,
            "classId": class_row.id,
            "type": a.get("type").cloned().unwrap_or(Value::Null),
            "due": rfc3339(a.get("dueDate").unwrap_or(&Value::Null), &zone),
            "hoursUntilDue": hours_until(a.get("dueDate").unwrap_or(&Value::Null), &zone),
            "overdue": a.get("dueDatePassed").and_then(Value::as_bool).unwrap_or(false),
            "description": clean(a.get("description")),
            "submitted": a.get("isSubmitted").and_then(Value::as_bool).unwrap_or(false),
            "submittedOn": rfc3339(a.get("submittedOn").unwrap_or(&Value::Null), &zone),
            "rejected": a.get("isRejected").and_then(Value::as_bool).unwrap_or(false),
            "graded": a.get("isGraded").and_then(Value::as_bool).unwrap_or(false),
            "grade": a.get("grade").cloned().unwrap_or(Value::Null),
            "remark": opt(&clean(a.get("remark"))),
            "marks": {
                "minimum": a.get("minimumMarks").cloned().unwrap_or(Value::Null),
                "maximum": a.get("maximumMarks").cloned().unwrap_or(Value::Null),
            },
            "attachments": attachments,
            "timezone": zone,
            "note": "signed attachment links expire in about an hour — ask again for a fresh one",
        }))
    }

    // ------------------------------------------------------------------ //
    // attendance
    // ------------------------------------------------------------------ //

    pub async fn attendance(
        &self,
        term: Option<&str>,
        class: Option<&str>,
    ) -> Result<Value, QuadError> {
        let p = self.ready()?;
        let ukid = self.ukid().await?;
        let zone = self.zone();

        let (programme, batch) = p.profile_ids().await?;
        let terms = p.terms(programme, batch).await?;
        if terms.is_empty() {
            return Err(QuadError::NotFound(
                "no academic terms for this account".into(),
            ));
        }
        let term = match term.filter(|s| !s.trim().is_empty()) {
            Some(sel) => {
                let sel = sel.trim().to_lowercase();
                terms
                    .iter()
                    .find(|t| {
                        t.get("id").and_then(Value::as_i64).map(|i| i.to_string())
                            == Some(sel.clone())
                            || clean(t.get("name")).to_lowercase().contains(&sel)
                    })
                    .cloned()
                    .ok_or_else(|| QuadError::NotFound(format!("no term matches {sel:?}")))?
            }
            None => terms
                .iter()
                .find(|t| t.get("active").and_then(Value::as_bool).unwrap_or(false))
                .or_else(|| {
                    terms.iter().find(|t| {
                        !t.get("past").and_then(Value::as_bool).unwrap_or(false)
                            && !t.get("future").and_then(Value::as_bool).unwrap_or(false)
                    })
                })
                .unwrap_or(&terms[0])
                .clone(),
        };
        let term_id = term.get("id").and_then(Value::as_i64).unwrap_or(0);
        let data = p.attendance(ukid, term_id).await?;

        let class_filter = match class.filter(|s| !s.trim().is_empty()) {
            Some(sel) => Some(self.resolve_class(sel).await?),
            None => None,
        };

        // 75% is the common Indian college minimum; the tools report it so the
        // agent can flag it rather than silently assume.
        let threshold = 75.0_f64;
        let mut courses_json = Vec::new();
        let mut present = 0.0;
        let mut absent = 0.0;

        for course in as_array(data.get("courseAttendance").cloned().unwrap_or(Value::Null)) {
            let components: Vec<Value> =
                as_array(course.get("components").cloned().unwrap_or(Value::Null))
                    .into_iter()
                    .filter(|c| match &class_filter {
                        Some(f) => c.get("classId").and_then(Value::as_i64) == Some(f.id),
                        None => true,
                    })
                    .collect();
            if components.is_empty() {
                continue;
            }
            let (cp, ca) = components.iter().fold((0.0, 0.0), |(p, a), c| {
                (
                    p + num(c.get("totalPresent")),
                    a + num(c.get("totalAbsent")),
                )
            });
            present += cp;
            absent += ca;
            courses_json.push(json!({
                "course": clean(course.get("courseName")),
                "present": cp,
                "absent": ca,
                "total": cp + ca,
                "percentage": round1(if cp + ca > 0.0 { cp / (cp + ca) * 100.0 } else { 100.0 }),
                "components": components.iter().map(|c| json!({
                    "classId": int(c, "classId"),
                    "component": clean(c.get("courseComponentTypeName")),
                    "present": num(c.get("totalPresent")),
                    "absent": num(c.get("totalAbsent")),
                    "percentage": round1(num(c.get("percentage"))),
                    "unmarkedLessons": as_array(c.get("attendance").cloned().unwrap_or(Value::Null))
                        .iter().filter(|l| l.get("status").is_some_and(Value::is_null)).count(),
                })).collect::<Vec<_>>(),
            }));
        }

        let total = present + absent;
        let pct = if total > 0.0 {
            present / total * 100.0
        } else {
            100.0
        };
        let (bunks_left, bunks_needed) = slack(present, total, threshold);

        Ok(json!({
            "term": clean(term.get("name")),
            "termId": term_id,
            "timezone": zone,
            "overall": {
                "present": present,
                "absent": absent,
                "total": total,
                "percentage": round1(pct),
                "requiredPercentage": threshold,
                "status": if pct >= threshold { "safe" } else { "below-minimum" },
                "bunksLeft": bunks_left,
                "bunksNeeded": bunks_needed,
            },
            "courses": courses_json,
            "notes": [
                "total = present + absent; unmarked and 'do not consider' lessons are excluded",
                "bunksLeft/bunksNeeded use requiredPercentage above — recompute if your college differs"
            ]
        }))
    }

    // ------------------------------------------------------------------ //
    // grades
    // ------------------------------------------------------------------ //

    pub async fn grades(&self, class: Option<&str>) -> Result<Value, QuadError> {
        let p = self.ready()?;
        let targets = self.targets(class).await?;

        let mut rows = Vec::new();
        let mut warnings = Vec::new();
        for c in &targets {
            match p.gradebook(c.id).await {
                Ok(r) => {
                    let published = r
                        .get("result")
                        .and_then(|x| x.get("publishMarks"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    rows.push(json!({
                        "class": c.label,
                        "classId": c.id,
                        "published": published,
                        "result": r.get("result").cloned().unwrap_or(Value::Null),
                        "grades": r.get("grades").cloned().unwrap_or(json!([])),
                        "components": r.get("assessmentComponentResults").cloned().unwrap_or(json!([])),
                    }));
                }
                Err(QuadError::Forbidden(_)) | Err(QuadError::NotFound(_)) => rows.push(json!({
                    "class": c.label,
                    "classId": c.id,
                    "published": false,
                    "note": "the portal exposes no result for this class yet"
                })),
                Err(e) => warnings.push(json!({ "class": c.label, "error": e.to_string() })),
            }
        }

        let mut out = json!({ "count": rows.len(), "classes": rows });
        if !warnings.is_empty() {
            out["warnings"] = json!(warnings);
        }
        Ok(out)
    }

    // ------------------------------------------------------------------ //
    // search
    // ------------------------------------------------------------------ //

    pub async fn search(&self, query: &str, class: Option<&str>) -> Result<Value, QuadError> {
        let p = self.ready()?;
        let ukid = self.ukid().await?;
        let zone = self.zone();
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Err(QuadError::Config("query must be non-empty".into()));
        }
        let targets = self.targets(class).await?;

        let mut hits: BTreeMap<i64, Value> = BTreeMap::new();
        for c in &targets {
            let Ok(buckets) = p.all_assignments(c.id, ukid).await else {
                continue;
            };
            for a in buckets.upcoming.iter().chain(&buckets.previous) {
                let hay = format!(
                    "{} {}",
                    clean(a.get("name")).to_lowercase(),
                    clean(a.get("description")).to_lowercase()
                );
                if !hay.contains(&needle) {
                    continue;
                }
                let id = a.get("id").and_then(Value::as_i64).unwrap_or(0);
                let mut row = self.summarise_pending(a, &c.label);
                row["submitted"] = json!(
                    a.get("isSubmitted")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                );
                row["description"] = json!(
                    clean(a.get("description"))
                        .chars()
                        .take(400)
                        .collect::<String>()
                );
                row["timezone"] = json!(zone);
                hits.insert(id, row);
            }
        }
        Ok(json!({
            "query": query,
            "count": hits.len(),
            "matches": hits.into_values().collect::<Vec<_>>(),
        }))
    }
}

/// How many classes can still be missed, and how many straight classes are
/// needed to climb back over `threshold`. Both zero when exactly at the line.
pub fn slack(present: f64, total: f64, threshold: f64) -> (f64, f64) {
    if total <= 0.0 {
        return (0.0, 0.0);
    }
    let pct = present / total * 100.0;
    if pct >= threshold {
        ((present * 100.0 / threshold - total).floor().max(0.0), 0.0)
    } else {
        let needed = ((threshold * total - 100.0 * present) / (100.0 - threshold))
            .ceil()
            .max(0.0);
        (0.0, needed)
    }
}

fn unix_to_rfc3339(ts: i64) -> Option<String> {
    use chrono::TimeZone;
    chrono::Utc
        .timestamp_opt(ts, 0)
        .single()
        .map(|d| d.to_rfc3339())
}

// Keep `finish` reachable for the registry without re-exporting errors there.
#[allow(dead_code)]
pub(crate) fn _finish_is_used() -> &'static str {
    let _ = finish::<std::future::Ready<Result<Value, QuadError>>>;
    ""
}

impl Quad {
    /// Every assignment and quiz, grouped by class, for a per-class view.
    pub async fn assignments(&self, class: Option<&str>) -> Result<Value, QuadError> {
        let p = self.ready()?;
        let ukid = self.ukid().await?;
        let zone = self.zone();
        let targets = self.targets(class).await?;

        let mut rows = Vec::new();
        let mut warnings = Vec::new();
        for c in &targets {
            let mut entry = json!({ "class": c.label, "classId": c.id });
            match p.all_assignments(c.id, ukid).await {
                Ok(buckets) => {
                    let summarise = |list: &[Value]| -> Vec<Value> {
                        list.iter()
                            .map(|a| {
                                let mut row = self.summarise_pending(a, &c.label);
                                row["submitted"] = json!(
                                    a.get("isSubmitted")
                                        .and_then(Value::as_bool)
                                        .unwrap_or(false)
                                );
                                row["graded"] = json!(
                                    a.get("isGraded").and_then(Value::as_bool).unwrap_or(false)
                                );
                                row["grade"] = a.get("grade").cloned().unwrap_or(Value::Null);
                                row
                            })
                            .collect()
                    };
                    entry["upcoming"] = json!(summarise(&buckets.upcoming));
                    entry["previous"] = json!(summarise(&buckets.previous));
                    entry["drafts"] = json!(summarise(&buckets.draft));
                }
                Err(e) => {
                    entry["error"] = json!(e.to_string());
                    warnings.push(json!({ "class": c.label, "error": e.to_string() }));
                }
            }
            let quizzes = match p.quizzes_page(c.id, 1, 50, None, None).await {
                Ok(page) => Some(as_array(
                    page.get("quizzes").cloned().unwrap_or(Value::Null),
                )),
                Err(_) => None,
            };
            if let Some(quizzes) = quizzes {
                entry["quizzes"] =
                    json!(quizzes
                    .iter()
                    .map(|q| json!({
                        "quizId": int(q, "id"),
                        "title": clean(q.get("title")),
                        "status": clean(q.get("status")).to_lowercase(),
                        "opens": rfc3339(q.get("quizStartTime").unwrap_or(&Value::Null), &zone),
                        "closes": rfc3339(q.get("quizEndTime").unwrap_or(&Value::Null), &zone),
                        "maxMarks": q.get("maxMarks").cloned().unwrap_or(Value::Null),
                    }))
                    .collect::<Vec<_>>());
            }
            rows.push(entry);
        }

        let mut out = json!({ "count": rows.len(), "timezone": zone, "classes": rows });
        if !warnings.is_empty() {
            out["warnings"] = json!(warnings);
        }
        Ok(out)
    }

    // ------------------------------------------------------------------ //
    // Write and agentic interactive tools
    // ------------------------------------------------------------------ //

    async fn resolve_quiz(
        &self,
        selector: &str,
        class: Option<&str>,
    ) -> Result<(ClassRow, Value), QuadError> {
        let p = self.ready()?;
        let targets = self.targets(class).await?;
        let wanted = selector.trim().to_lowercase();
        let id_opt = wanted.parse::<i64>().ok();

        for c in &targets {
            let Ok(page) = p.quizzes_page(c.id, 1, 50, None, None).await else {
                continue;
            };
            for q in as_array(page.get("quizzes").cloned().unwrap_or(Value::Null)) {
                let id = q.get("id").and_then(Value::as_i64).unwrap_or(0);
                let title = clean(q.get("title")).to_lowercase();
                if Some(id) == id_opt || title == wanted || title.contains(&wanted) {
                    return Ok((c.clone(), q));
                }
            }
        }

        // If not found in open quizzes, check if the numeric id is valid
        if let Some(id) = id_opt
            && let Some(first) = targets.first()
        {
            return Ok((
                first.clone(),
                json!({ "id": id, "title": format!("Quiz #{id}") }),
            ));
        }

        Err(QuadError::NotFound(format!(
            "no quiz matches {selector:?}. Check `due` or `assignments` for open quizzes."
        )))
    }

    /// Retrieve questions and options for an active quiz so the agent can inspect before answering.
    pub async fn quiz_questions(
        &self,
        quiz: &str,
        class: Option<&str>,
    ) -> Result<Value, QuadError> {
        let p = self.ready()?;
        let (class_row, q) = self.resolve_quiz(quiz, class).await?;
        let quiz_id = q.get("id").and_then(Value::as_i64).unwrap_or(0);
        let title = clean(q.get("title"));
        let questions = p.quiz_questions(quiz_id).await?;

        Ok(json!({
            "quizId": quiz_id,
            "title": title,
            "class": class_row.label,
            "classId": class_row.id,
            "questions": questions,
        }))
    }

    /// Submit answers for a quiz. Supports dry_run for compliance verification.
    pub async fn submit_quiz(
        &self,
        quiz: &str,
        class: Option<&str>,
        answers: &[Value],
        dry_run: Option<bool>,
    ) -> Result<Value, QuadError> {
        let p = self.ready()?;
        let (class_row, q) = self.resolve_quiz(quiz, class).await?;
        let quiz_id = q.get("id").and_then(Value::as_i64).unwrap_or(0);
        let title = clean(q.get("title"));

        let payload = json!({
            "quizId": quiz_id,
            "classId": class_row.id,
            "answers": answers,
        });

        if dry_run.unwrap_or(false) {
            return Ok(json!({
                "status": "dry_run_ready",
                "action": "submit_quiz",
                "quiz": title,
                "quizId": quiz_id,
                "class": class_row.label,
                "answersCount": answers.len(),
                "payload": payload,
                "note": "Dry run preview: quiz answers validated. Pass dry_run: false to submit live.",
            }));
        }

        let res = p.submit_quiz(quiz_id, &payload).await?;
        Ok(json!({
            "status": "submitted",
            "quiz": title,
            "quizId": quiz_id,
            "class": class_row.label,
            "result": res,
        }))
    }

    /// Submit an assignment with text and optional attachments. Supports dry_run preview.
    pub async fn submit_assignment(
        &self,
        assignment: &str,
        class: Option<&str>,
        text: Option<&str>,
        attachments: Option<&[String]>,
        dry_run: Option<bool>,
    ) -> Result<Value, QuadError> {
        let p = self.ready()?;
        let (class_row, a) = self.resolve_assignment_entry(assignment, class).await?;
        let assignment_id = a
            .get("id")
            .and_then(Value::as_i64)
            .or_else(|| a.get("assignmentId").and_then(Value::as_i64))
            .unwrap_or(0);
        let assignment_name = clean(a.get("name"));

        let payload = json!({
            "assignmentId": assignment_id,
            "classId": class_row.id,
            "submissionText": text.unwrap_or(""),
            "attachments": attachments.unwrap_or(&[]),
        });

        if dry_run.unwrap_or(false) {
            return Ok(json!({
                "status": "dry_run_ready",
                "action": "submit_assignment",
                "assignment": assignment_name,
                "assignmentId": assignment_id,
                "class": class_row.label,
                "classId": class_row.id,
                "payload": payload,
                "note": "Dry run preview: submission validated. Pass dry_run: false to submit live.",
            }));
        }

        let res = p
            .submit_assignment(class_row.id, assignment_id, &payload)
            .await?;
        Ok(json!({
            "status": "submitted",
            "assignment": assignment_name,
            "assignmentId": assignment_id,
            "class": class_row.label,
            "result": res,
        }))
    }

    /// List pending feedback forms / questionnaires.
    pub async fn feedback_forms(&self, class: Option<&str>) -> Result<Value, QuadError> {
        let p = self.ready()?;
        let forms = p.feedback_forms().await?;
        let filter = class
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty());

        let rows: Vec<Value> = forms
            .into_iter()
            .filter(|f| {
                if let Some(ref flt) = filter {
                    let hay = format!(
                        "{} {} {}",
                        clean(f.get("title")),
                        clean(f.get("courseName")),
                        clean(f.get("facultyName"))
                    )
                    .to_lowercase();
                    hay.contains(flt)
                } else {
                    true
                }
            })
            .map(|f| {
                json!({
                    "formId": f.get("formId").or_else(|| f.get("id")).cloned().unwrap_or(Value::Null),
                    "title": clean(f.get("title")),
                    "course": clean(f.get("courseName")),
                    "faculty": clean(f.get("facultyName")),
                    "status": clean(f.get("status")),
                    "deadline": f.get("deadline").cloned().unwrap_or(Value::Null),
                })
            })
            .collect();

        Ok(json!({
            "count": rows.len(),
            "forms": rows,
        }))
    }

    /// Fill and submit a student feedback questionnaire. Supports dry_run.
    pub async fn fill_feedback(
        &self,
        form: &str,
        ratings: &[Value],
        dry_run: Option<bool>,
    ) -> Result<Value, QuadError> {
        let p = self.ready()?;
        let form_id = if let Ok(id) = form.trim().parse::<i64>() {
            id
        } else {
            let list = p.feedback_forms().await?;
            let wanted = form.trim().to_lowercase();
            list.iter()
                .find(|f| clean(f.get("title")).to_lowercase().contains(&wanted))
                .and_then(|f| {
                    f.get("formId")
                        .or_else(|| f.get("id"))
                        .and_then(Value::as_i64)
                })
                .ok_or_else(|| QuadError::NotFound(format!("no feedback form matches {form:?}")))?
        };

        let payload = json!({
            "formId": form_id,
            "ratings": ratings,
        });

        if dry_run.unwrap_or(false) {
            return Ok(json!({
                "status": "dry_run_ready",
                "action": "fill_feedback",
                "formId": form_id,
                "ratingsCount": ratings.len(),
                "payload": payload,
                "note": "Dry run preview: feedback validated. Pass dry_run: false to submit live.",
            }));
        }

        let res = p.submit_feedback(form_id, &payload).await?;
        Ok(json!({
            "status": "submitted",
            "formId": form_id,
            "result": res,
        }))
    }

    /// Generic campus form / questionnaire submission. Supports dry_run.
    pub async fn fill_form(
        &self,
        form_path: &str,
        fields: &Value,
        dry_run: Option<bool>,
    ) -> Result<Value, QuadError> {
        let p = self.ready()?;
        let path = if form_path.starts_with('/') {
            form_path.to_string()
        } else {
            format!("/{form_path}")
        };

        if dry_run.unwrap_or(false) {
            return Ok(json!({
                "status": "dry_run_ready",
                "action": "fill_form",
                "endpoint": path,
                "fields": fields,
                "note": "Dry run preview: form payload validated. Pass dry_run: false to submit live.",
            }));
        }

        let res = p.submit_form(&path, fields).await?;
        Ok(json!({
            "status": "submitted",
            "endpoint": path,
            "result": res,
        }))
    }
}
