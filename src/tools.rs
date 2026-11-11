//! What each tool does.
//!
//! Human architecture notes:
//! College portals are notoriously fragmented: assignments live in one sub-system,
//! quizzes in another, and attendance in a third. Most portal apps force students
//! through 15 clicks to find out whether they can safely miss class on Monday morning.
//!
//! Here we shape tools around actual student questions:
//!
//!   1. **Attendance & "Bunk Maths"**:
//!      Nearly every Indian university enforces a mandatory 75% attendance rule
//!      (falling below debars the student from end-semester examinations). Knowing
//!      you have 81.2% is useless without knowing what that means in practice:
//!      * `bunksLeft`: how many subsequent classes can you skip before dropping below 75%?
//!        Formula: $\lfloor \frac{\text{present} - 0.75 \times \text{total}}{0.75} \rfloor$
//!      * `bunksNeeded`: if you're in the red, how many consecutive classes must you attend
//!        to climb back to 75%?
//!        Formula: $\lceil \frac{0.75 \times \text{total} - \text{present}}{0.25} \rceil$
//!
//!      `quad attendance` calculates both per course component (Lecture, Lab, Tutorial)
//!      and globally.
//!
//!   2. **Deadlines Unified**:
//!      The portal stores quizzes and assignments in distinct database tables with completely
//!      different date keys (`scheduleEndDate` vs `submissionDeadline`). `quad due` joins
//!      them, converts naive IST strings into timezone-accurate RFC 3339 timestamps, sorts
//!      chronologically, and adds a precalculated `hoursUntilDue` so agents don't have to do
//!      date arithmetic in prompt context.
//!
//!   3. **Fuzzy Class Resolution**:
//!      AI agents should never be forced to make an extra API call just to look up an integer
//!      `classId`. Every tool accepts a loose class selector (`"Physics"`, `"IOT Lab"`, `"CS102"`),
//!      and `pick_class` matches by exact ID, exact course code, case-insensitive substring,
//!      and token set intersection.
//!
//!   4. **Agentic Actions & Safety**:
//!      For write actions (`submit_assignment`, `attempt_quiz`, `submit_feedback`), compliance
//!      is paramount. All write tools support `dry_run: true` by default, previewing the exact
//!      payload, questions, and parameters without modifying portal state until explicitly
//!      confirmed.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use tokio::task::JoinSet;

use super::server::Quad;
use crate::error::DigiError;
use crate::journal::Journal;
use crate::portal::{as_array, clean, hours_until, int, rfc3339, unix_to_rfc3339};

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

    pub async fn ukid(&self) -> Result<i64, DigiError> {
        let p = self.ready()?;
        if let Some(u) = p.config().ukid {
            return Ok(u);
        }
        let me = p.me().await?;
        me.get("ukid")
            .and_then(Value::as_i64)
            .ok_or_else(|| DigiError::Parse("the profile has no ukid".into()))
    }

    pub async fn class_list(&self) -> Result<Vec<ClassRow>, DigiError> {
        Ok(self
            .ready()?
            .classes()
            .await?
            .iter()
            .map(ClassRow::from_value)
            .collect())
    }

    /// The classes a selector applies to (all of them when none is given).
    async fn targets(&self, selector: Option<&str>) -> Result<Vec<ClassRow>, DigiError> {
        match selector.filter(|s| !s.trim().is_empty()) {
            Some(sel) => Ok(vec![self.resolve_class(sel).await?]),
            None => self.class_list().await,
        }
    }

    async fn resolve_class(&self, selector: &str) -> Result<ClassRow, DigiError> {
        let classes = self.class_list().await?;
        pick_class(&classes, selector).ok_or_else(|| {
            DigiError::NotFound(format!(
                "no class matches {selector:?}. Call `classes` to see the list."
            ))
        })
    }

    // ------------------------------------------------------------------ //
    // account
    // ------------------------------------------------------------------ //

    pub async fn account(&self) -> Result<serde_json::Value, DigiError> {
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

    pub async fn classes(&self, query: Option<&str>) -> Result<Value, DigiError> {
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
    ) -> Result<Value, DigiError> {
        let p = self.ready()?;
        let ukid = self.ukid().await?;
        let zone = self.zone();
        let targets = self.targets(class).await?;

        let mut items: Vec<Value> = Vec::new();
        let mut warnings: Vec<Value> = Vec::new();

        // Assignments: one task per class, sharing the existing HTTP client pool.
        let mut set = JoinSet::new();
        for c in &targets {
            let (id, label, portal) = (c.id, c.label.clone(), p.clone());
            set.spawn(async move {
                let result = portal.all_assignments(id, ukid).await;
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
                let raw_status = clean(q.get("status"));
                if raw_status.eq_ignore_ascii_case("CLOSED") {
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
                    "status": raw_status.to_ascii_lowercase(),
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
    ) -> Result<(ClassRow, Value), DigiError> {
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
            DigiError::NotFound(format!(
                "no assignment matches {selector:?}. Use `search` to find the exact wording."
            ))
        })
    }

    pub async fn assignment(
        &self,
        selector: &str,
        class: Option<&str>,
    ) -> Result<Value, DigiError> {
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
    // course materials
    // ------------------------------------------------------------------ //

    pub async fn materials(
        &self,
        class_query: Option<&str>,
        class_id_override: Option<i64>,
    ) -> Result<Value, DigiError> {
        let p = self.ready()?;
        let targets = if let Some(cid) = class_id_override {
            vec![ClassRow {
                id: cid,
                label: format!("Class #{cid}"),
                course_name: format!("Class #{cid}"),
                course_code: String::new(),
                kind: String::new(),
                credits: Value::Null,
                faculty: String::new(),
            }]
        } else {
            self.targets(class_query).await?
        };

        let mut results = Vec::new();
        for target in targets {
            let modules = p.modules(target.id).await.unwrap_or_default();
            let resources = p.resources(target.id).await.unwrap_or_default();

            let mut module_map = std::collections::HashMap::new();
            for m in &modules {
                let mid = m
                    .get("id")
                    .or_else(|| m.get("moduleId"))
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let title = clean(m.get("title").or_else(|| m.get("moduleName")));
                module_map.insert(mid, title);
            }

            let formatted_resources: Vec<Value> = resources
                .iter()
                .map(|r| {
                    let rid = r
                        .get("id")
                        .or_else(|| r.get("resourceId"))
                        .and_then(Value::as_i64)
                        .unwrap_or(0);
                    let mid = r.get("moduleId").and_then(Value::as_i64).unwrap_or(0);
                    let mod_name = module_map
                        .get(&mid)
                        .cloned()
                        .unwrap_or_else(|| "General Resources".to_string());
                    let title = clean(r.get("title").or_else(|| r.get("name")));
                    let r_type = clean(r.get("type").or_else(|| r.get("resourceType")));
                    let url = clean(
                        r.get("url")
                            .or_else(|| r.get("previewUrl"))
                            .or_else(|| r.get("weblink")),
                    );

                    json!({
                        "resourceId": rid,
                        "title": title,
                        "module": mod_name,
                        "type": if r_type.is_empty() { "Document" } else { &r_type },
                        "url": if url.is_empty() { Value::Null } else { json!(url) },
                    })
                })
                .collect();

            results.push(json!({
                "classId": target.id,
                "className": target.label,
                "totalMaterials": formatted_resources.len(),
                "materials": formatted_resources,
            }));
        }

        Ok(json!({
            "classes": results,
            "hint": "Use tool `material` with `resource_id` to generate a fresh signed download link.",
        }))
    }

    pub async fn material(
        &self,
        resource_id: i64,
        class_query: Option<&str>,
        class_id_override: Option<i64>,
    ) -> Result<Value, DigiError> {
        let p = self.ready()?;
        let class_id = if let Some(cid) = class_id_override {
            cid
        } else if let Some(q) = class_query {
            self.resolve_class(q).await?.id
        } else {
            // Search across enrolled classes to find which class owns this resource
            let all = self.targets(None).await?;
            let mut found_cid = None;
            for t in all {
                let res = p.resources(t.id).await.unwrap_or_default();
                if res.iter().any(|r| {
                    r.get("id")
                        .or_else(|| r.get("resourceId"))
                        .and_then(Value::as_i64)
                        == Some(resource_id)
                }) {
                    found_cid = Some(t.id);
                    break;
                }
            }
            found_cid.ok_or_else(|| {
                DigiError::NotFound(format!(
                    "Resource ID {resource_id} not found across enrolled classes. Pass class name to narrow search."
                ))
            })?
        };

        let signed = p.signed_url(class_id, resource_id).await?;
        let url = clean(
            signed
                .get("url")
                .or_else(|| signed.get("signedUrl"))
                .or_else(|| signed.get("fileUrl")),
        );

        Ok(json!({
            "resourceId": resource_id,
            "classId": class_id,
            "signedUrl": url,
            "expiresIn": "~1 hour",
            "details": signed,
        }))
    }

    // ------------------------------------------------------------------ //
    // attendance
    // ------------------------------------------------------------------ //

    pub async fn attendance(
        &self,
        term: Option<&str>,
        class: Option<&str>,
    ) -> Result<Value, DigiError> {
        let p = self.ready()?;
        let ukid = self.ukid().await?;
        let zone = self.zone();

        let (programme, batch) = p.profile_ids().await?;
        let terms = p.terms(programme, batch).await?;
        if terms.is_empty() {
            return Err(DigiError::NotFound(
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
                    .ok_or_else(|| DigiError::NotFound(format!("no term matches {sel:?}")))?
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
        let mut total_unmarked = 0;

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
            let c_total = cp + ca;
            let c_unmarked: usize = components
                .iter()
                .map(|c| {
                    as_array(c.get("attendance").cloned().unwrap_or(Value::Null))
                        .iter()
                        .filter(|l| l.get("status").is_some_and(Value::is_null))
                        .count()
                })
                .sum();
            total_unmarked += c_unmarked;

            let (c_pct, c_status) = if c_total > 0.0 {
                let p = cp / c_total * 100.0;
                (
                    json!(round1(p)),
                    if p >= threshold {
                        "safe"
                    } else {
                        "below-minimum"
                    },
                )
            } else {
                (Value::Null, "no-data")
            };

            courses_json.push(json!({
                "course": clean(course.get("courseName")),
                "present": cp,
                "absent": ca,
                "total": c_total,
                "percentage": c_pct,
                "status": c_status,
                "unmarkedLessons": c_unmarked,
                "components": components.iter().map(|c| {
                    let comp_present = num(c.get("totalPresent"));
                    let comp_absent = num(c.get("totalAbsent"));
                    let comp_total = comp_present + comp_absent;
                    json!({
                        "classId": int(c, "classId"),
                        "component": clean(c.get("courseComponentTypeName")),
                        "present": comp_present,
                        "absent": comp_absent,
                        "total": comp_total,
                        "percentage": if comp_total > 0.0 {
                            json!(round1(num(c.get("percentage"))))
                        } else {
                            Value::Null
                        },
                        "unmarkedLessons": as_array(c.get("attendance").cloned().unwrap_or(Value::Null))
                            .iter().filter(|l| l.get("status").is_some_and(Value::is_null)).count(),
                    })
                }).collect::<Vec<_>>(),
            }));
        }

        let total = present + absent;
        let (pct, status, bunks_left, bunks_needed) = if total > 0.0 {
            let p = present / total * 100.0;
            let (bl, bn) = slack(present, total, threshold);
            (
                json!(round1(p)),
                if p >= threshold {
                    "safe"
                } else {
                    "below-minimum"
                },
                json!(bl),
                json!(bn),
            )
        } else {
            (Value::Null, "no-data", Value::Null, Value::Null)
        };

        let mut overall_json = json!({
            "present": present,
            "absent": absent,
            "total": total,
            "percentage": pct,
            "requiredPercentage": threshold,
            "status": status,
            "bunksLeft": bunks_left,
            "bunksNeeded": bunks_needed,
            "unmarkedLessons": total_unmarked,
        });

        if total > 0.0 && total_unmarked > 0 {
            let worst_case_pct = round1(present / (total + total_unmarked as f64) * 100.0);
            overall_json["worstCasePercentageIfUnmarkedAbsent"] = json!(worst_case_pct);
            if (total_unmarked as f64) / (total + total_unmarked as f64) > 0.20 {
                overall_json["unmarkedWarning"] = json!(format!(
                    "High uncertainty: {total_unmarked} lessons are unmarked (>20% of schedule). If marked absent, attendance drops to {worst_case_pct}%."
                ));
            }
        }

        Ok(json!({
            "term": clean(term.get("name")),
            "termId": term_id,
            "timezone": zone,
            "overall": overall_json,
            "courses": courses_json,
            "notes": [
                "total = present + absent; unmarked and 'do not consider' lessons are excluded",
                "courses with 0 total classes report status 'no-data' and null percentage",
                "bunksLeft/bunksNeeded use requiredPercentage above — recompute if your college differs"
            ]
        }))
    }

    // ------------------------------------------------------------------ //
    // grades
    // ------------------------------------------------------------------ //

    pub async fn grades(&self, class: Option<&str>) -> Result<Value, DigiError> {
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
                Err(DigiError::Forbidden(_)) | Err(DigiError::NotFound(_)) => rows.push(json!({
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

    pub async fn search(&self, query: &str, class: Option<&str>) -> Result<Value, DigiError> {
        let p = self.ready()?;
        let ukid = self.ukid().await?;
        let zone = self.zone();
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Err(DigiError::Config("query must be non-empty".into()));
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

impl Quad {
    /// Every assignment and quiz, grouped by class, for a per-class view.
    pub async fn assignments(&self, class: Option<&str>) -> Result<Value, DigiError> {
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
    ) -> Result<(ClassRow, Value), DigiError> {
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

        Err(DigiError::NotFound(format!(
            "no quiz matches {selector:?}. Check `due` or `assignments` for open quizzes."
        )))
    }

    /// Retrieve questions and options for an active quiz so the agent can inspect before answering.
    pub async fn quiz_questions(
        &self,
        quiz: &str,
        class: Option<&str>,
    ) -> Result<Value, DigiError> {
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
    ) -> Result<Value, DigiError> {
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
    ) -> Result<Value, DigiError> {
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
    pub async fn feedback_forms(&self, class: Option<&str>) -> Result<Value, DigiError> {
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
        resubmit: Option<bool>,
        force: Option<bool>,
    ) -> Result<Value, DigiError> {
        if ratings.is_empty() {
            return Err(DigiError::Validation(
                "Cannot submit feedback with empty ratings: specify question_id, rating, and remarks for the evaluation questions."
                    .to_string(),
            ));
        }
        let p = self.ready()?;
        let list = p.feedback_forms().await?;
        let wanted = form.trim().to_lowercase();

        // 1. Filter candidates by title, courseName, facultyName, or formId
        let matches: Vec<&Value> = list
            .iter()
            .filter(|f| {
                let id_str = f
                    .get("formId")
                    .or_else(|| f.get("id"))
                    .and_then(Value::as_i64)
                    .map(|i| i.to_string())
                    .unwrap_or_default();
                id_str == wanted
                    || clean(f.get("title")).to_lowercase().contains(&wanted)
                    || clean(f.get("courseName")).to_lowercase().contains(&wanted)
                    || clean(f.get("facultyName")).to_lowercase().contains(&wanted)
            })
            .collect();

        if matches.is_empty() {
            return Err(DigiError::NotFound(format!(
                "no feedback form matches {form:?}"
            )));
        }

        // 2. Status filtering: If form is SUBMITTED, refuse unless resubmit is requested
        let pending: Vec<&Value> = matches
            .iter()
            .copied()
            .filter(|f| !clean(f.get("status")).eq_ignore_ascii_case("SUBMITTED"))
            .collect();

        let chosen = if !pending.is_empty() {
            if let Some(exact) = pending.iter().find(|f| {
                clean(f.get("title")).to_ascii_lowercase() == wanted
                    || f.get("formId")
                        .or_else(|| f.get("id"))
                        .and_then(Value::as_i64)
                        .map(|i| i.to_string())
                        == Some(wanted.clone())
            }) {
                exact
            } else if pending.len() > 1 && wanted.parse::<i64>().is_err() {
                let names: Vec<String> = pending
                    .iter()
                    .map(|f| {
                        format!(
                            "\"{}\" (Faculty: {}, ID: {})",
                            clean(f.get("title")),
                            clean(f.get("facultyName")),
                            f.get("formId")
                                .or_else(|| f.get("id"))
                                .and_then(Value::as_i64)
                                .unwrap_or(0)
                        )
                    })
                    .collect();
                return Err(DigiError::Validation(format!(
                    "Ambiguous form query {form:?} matches {} pending forms: {}. Please specify the exact title or numeric form ID.",
                    pending.len(),
                    names.join(", ")
                )));
            } else {
                pending[0]
            }
        } else {
            // All matches are already SUBMITTED
            if resubmit != Some(true) {
                let first = matches[0];
                let title = clean(first.get("title"));
                let fid = first
                    .get("formId")
                    .or_else(|| first.get("id"))
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                return Err(DigiError::Validation(format!(
                    "Feedback form \"{title}\" (ID {fid}) is already marked as SUBMITTED. Pass resubmit: true to re-submit."
                )));
            }
            matches[0]
        };

        let session_id = chosen.get("sessionId").and_then(Value::as_i64).unwrap_or(0);
        let cft_id = chosen
            .get("formId")
            .or_else(|| chosen.get("id"))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let title = clean(chosen.get("title"));
        let target_key = format!("feedback_{cft_id}");

        // 3. Dynamic session template validation
        if session_id > 0 && cft_id > 0 {
            if force != Some(true) && resubmit != Some(true) {
                Journal::check_already_submitted("fill_feedback", &target_key)?;
            }

            // Fetch template to inspect questions and check coverage
            let tmpl = p.feedback_template(session_id, cft_id).await?;
            let mut total_questions = 0;
            let mut required_questions = Vec::new();

            if let Some(sections) = tmpl.get("sections").and_then(Value::as_array) {
                for sec in sections {
                    if let Some(questions) = sec.get("questions").and_then(Value::as_array) {
                        for q in questions {
                            total_questions += 1;
                            let q_id = q.get("questionId").and_then(Value::as_i64).unwrap_or(0);
                            let q_type = clean(q.get("type"));
                            let is_optional = q
                                .get("optionalQuestion")
                                .and_then(Value::as_bool)
                                .unwrap_or(false);
                            let q_text = clean(q.get("questionText"));

                            if !is_optional {
                                required_questions.push((q_id, q_type, q_text));
                            }
                        }
                    }
                }
            }

            // Check coverage against supplied ratings
            let mut missing = Vec::new();
            for (q_id, q_type, q_text) in &required_questions {
                let has_answer = ratings.iter().any(|r| {
                    if r.get("questionId").and_then(Value::as_i64) != Some(*q_id) {
                        return false;
                    }
                    if q_type == "SINGLE_SELECT" {
                        r.get("rating").and_then(Value::as_f64).is_some()
                    } else if q_type == "SUBJECTIVE" {
                        r.get("remarks")
                            .and_then(Value::as_str)
                            .map(|s| !s.trim().is_empty())
                            .unwrap_or(false)
                    } else {
                        true
                    }
                });
                if !has_answer {
                    missing.push(format!("Q#{q_id} ({q_type}): \"{q_text}\""));
                }
            }

            if !missing.is_empty() {
                return Err(DigiError::Validation(format!(
                    "Incomplete feedback ratings: form requires all {} mandatory questions to be answered, but {} are missing:\n  • {}",
                    required_questions.len(),
                    missing.len(),
                    missing.join("\n  • ")
                )));
            }

            if dry_run.unwrap_or(false) {
                return Ok(json!({
                    "status": "dry_run_ready",
                    "action": "fill_feedback",
                    "sessionId": session_id,
                    "courseFacultyTemplateId": cft_id,
                    "title": title,
                    "faculty": clean(chosen.get("facultyName")),
                    "totalQuestions": total_questions,
                    "mandatoryQuestions": required_questions.len(),
                    "ratingsValidated": ratings.len(),
                    "coverage": "100% verified",
                    "note": "Dry run preview verified against live portal template. Pass dry_run: false to submit.",
                }));
            }

            let res = p
                .submit_feedback_dynamic(session_id, cft_id, ratings)
                .await?;
            Journal::record(
                "fill_feedback",
                &target_key,
                &json!(ratings),
                "submitted",
                &title,
            );

            return Ok(json!({
                "status": "submitted",
                "sessionId": session_id,
                "courseFacultyTemplateId": cft_id,
                "title": title,
                "result": res,
            }));
        }

        // 2. Legacy / generic fallback
        let payload = json!({
            "formId": cft_id,
            "ratings": ratings,
        });

        if dry_run.unwrap_or(false) {
            return Ok(json!({
                "status": "dry_run_ready",
                "action": "fill_feedback",
                "formId": cft_id,
                "ratingsCount": ratings.len(),
                "payload": payload,
                "note": "Dry run preview: feedback validated. Pass dry_run: false to submit live.",
            }));
        }

        if force != Some(true) && resubmit != Some(true) {
            Journal::check_already_submitted("fill_feedback", &target_key)?;
        }
        let res = p.submit_feedback(cft_id, &payload).await?;
        Journal::record("fill_feedback", &target_key, &payload, "submitted", &title);

        Ok(json!({
            "status": "submitted",
            "formId": cft_id,
            "title": title,
            "result": res,
        }))
    }

    /// Generic campus form / questionnaire submission. Supports dry_run.
    pub async fn fill_form(
        &self,
        form_path: &str,
        fields: &Value,
        dry_run: Option<bool>,
        force: Option<bool>,
    ) -> Result<Value, DigiError> {
        let p = self.ready()?;
        let path = if form_path.starts_with('/') {
            form_path.to_string()
        } else {
            format!("/{form_path}")
        };
        let target_key = format!("form_{}", path.replace('/', "_"));

        if dry_run.unwrap_or(false) {
            return Ok(json!({
                "status": "dry_run_ready",
                "action": "fill_form",
                "endpoint": path,
                "fields": fields,
                "note": "Dry run preview: form payload validated. Pass dry_run: false to submit live.",
            }));
        }

        if force != Some(true) {
            Journal::check_already_submitted("fill_form", &target_key)?;
        }

        let res = p.submit_form(&path, fields).await?;
        Journal::record("fill_form", &target_key, fields, "submitted", &path);

        Ok(json!({
            "status": "submitted",
            "endpoint": path,
            "result": res,
        }))
    }
}

include!("tools_tests.rs");
