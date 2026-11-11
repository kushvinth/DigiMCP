//! Append-only write journal and idempotency protection for agentic actions.
//!
//! Rationale:
//! AI agents may crash, retry on timeouts, or re-run workflows in loops.
//! Irreversible write actions (submitting feedback forms, quizzes, assignments)
//! must have an audit trail and prevent accidental double-submits.

use std::fs::OpenOptions;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::config::data_dir;
use crate::error::DigiError;

pub struct Journal;

impl Journal {
    fn path() -> Option<PathBuf> {
        let dir = data_dir()?;
        let _ = std::fs::create_dir_all(&dir);
        Some(dir.join("journal.jsonl"))
    }

    /// Check if this tool + target was already submitted. If so, refuse unless forced.
    pub fn check_already_submitted(tool: &str, target: &str) -> Result<(), DigiError> {
        let Some(p) = Self::path() else {
            return Ok(());
        };
        if !p.exists() {
            return Ok(());
        }
        let Ok(contents) = std::fs::read_to_string(&p) else {
            return Ok(());
        };
        for line in contents.lines() {
            if let Ok(entry) = serde_json::from_str::<Value>(line)
                && entry.get("tool").and_then(Value::as_str) == Some(tool)
                && entry.get("target").and_then(Value::as_str) == Some(target)
                && entry.get("status").and_then(Value::as_str) == Some("submitted")
            {
                let ts = entry
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .unwrap_or("earlier");
                return Err(DigiError::Validation(format!(
                    "Action \"{tool}\" for target \"{target}\" was already recorded as submitted at {ts}. \
                     Duplicate write prevented. If you intentionally wish to overwrite, pass `force: true` or `resubmit: true`."
                )));
            }
        }
        Ok(())
    }

    /// Append a write record to the journal.
    pub fn record(tool: &str, target: &str, payload: &Value, status: &str, details: &str) {
        let Some(p) = Self::path() else {
            return;
        };
        let now = crate::portal::unix_to_rfc3339(crate::config::now_unix()).unwrap_or_default();
        let payload_str = payload.to_string();
        let hash = format!("{:016x}", calculate_hash(&payload_str));
        let entry = json!({
            "timestamp": now,
            "tool": tool,
            "target": target,
            "payload_hash": hash,
            "status": status,
            "details": details,
        });
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&p) {
            let _ = writeln!(file, "{}", entry);
        }
    }
}

fn calculate_hash(s: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}
