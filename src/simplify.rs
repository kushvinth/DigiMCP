//! Output shaping.
//!
//! Tool results are re-read by the model on every turn they stay in context, so
//! a verbose answer is paid for twice: once to fetch, once every time it is
//! recalled. Two cheap wins, applied to every successful result:
//!
//!   1. **Compact JSON.** `to_string_pretty` is for humans; the model does not
//!      benefit from indentation but does pay for it, roughly 20-30% of a
//!      typical payload in whitespace.
//!   2. **Null pruning.** Optional fields that the portal left empty
//!      (`"remark": null`) carry no information. Dropping them is lossless for
//!      the reader — absent and null mean "nothing there" — and keeps a `due`
//!      list with 30 items from repeating a dozen `null`s each.
//!
//! Anything that fails to reparse is returned untouched: shaping must never be
//! able to corrupt an answer.

use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::Value;

pub fn try_reduce(result: CallToolResult) -> CallToolResult {
    let mut out = result;
    let reduced: Vec<ContentBlock> = std::mem::take(&mut out.content)
        .into_iter()
        .map(|block| match block {
            ContentBlock::Text(text) => match serde_json::from_str::<Value>(&text.text) {
                Ok(value) => {
                    ContentBlock::text(serde_json::to_string(&prune(value)).unwrap_or(text.text))
                }
                Err(_) => ContentBlock::Text(text),
            },
            other => other,
        })
        .collect();
    out.content = reduced;
    out
}

/// Remove null object fields recursively. Empty arrays are kept: "no classes"
/// is information, whereas `null` is just an unfilled slot.
pub fn prune(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k, prune(v)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(prune).collect()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn drops_absent_fields_but_keeps_empty_lists() {
        let input = json!({
            "name": "Linked list lab",
            "remark": null,
            "attachments": [],
            "nested": { "grade": null, "marks": 0 },
            "items": [{ "a": null, "b": 1 }]
        });
        assert_eq!(
            prune(input),
            json!({
                "name": "Linked list lab",
                "attachments": [],
                "nested": { "marks": 0 },
                "items": [{ "b": 1 }]
            })
        );
    }

    #[test]
    fn reduces_whitespace_without_losing_data() {
        let result = CallToolResult::success(vec![ContentBlock::text(
            "{\n  \"count\": 2,\n  \"items\": [\n    1\n  ]\n}",
        )]);
        let out = try_reduce(result);
        let ContentBlock::Text(text) = &out.content[0] else {
            panic!("expected text")
        };
        assert_eq!(text.text, r#"{"count":2,"items":[1]}"#);
    }

    #[test]
    fn leaves_plain_text_alone() {
        let result = CallToolResult::error(vec![ContentBlock::text("Session expired.")]);
        let out = try_reduce(result);
        let ContentBlock::Text(text) = &out.content[0] else {
            panic!("expected text")
        };
        assert_eq!(text.text, "Session expired.");
    }
}
