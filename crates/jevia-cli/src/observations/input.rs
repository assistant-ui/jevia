//! Bounded envelopes, tiny retained metadata. Borrow raw field spans rather than
//! constructing JSON trees for tool output, prompts, or malformed identifier values.
use std::io::Read;

use jevia_core::valid_identifier;
use serde::Deserialize;
use serde_json::{Map, Value, value::RawValue};

use super::INPUT_LIMIT;

#[derive(Default, Deserialize)]
#[serde(default)]
struct Metadata<'a> {
    #[serde(borrow)]
    hook_event_name: Option<&'a RawValue>,
    #[serde(borrow)]
    session_id: Option<&'a RawValue>,
    #[serde(borrow)]
    agent_id: Option<&'a RawValue>,
    #[serde(borrow)]
    model: Option<&'a RawValue>,
    #[serde(borrow)]
    from_model: Option<&'a RawValue>,
    #[serde(borrow)]
    to_model: Option<&'a RawValue>,
    #[serde(borrow)]
    tool_name: Option<&'a RawValue>,
}

pub(super) fn metadata(input: impl Read) -> Option<Value> {
    let mut bytes = Vec::new();
    input.take(INPUT_LIMIT + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > INPUT_LIMIT {
        return None;
    }
    // Ignored JSON strings need not be decoded by serde; still reject invalid
    // UTF-8 anywhere in the envelope, not just in the fields we keep.
    let raw = std::str::from_utf8(&bytes).ok()?;
    // Derived structs also accept positional arrays; native envelopes must be objects.
    if !raw.trim_start().starts_with('{') {
        return None;
    }
    let fields: Metadata<'_> = serde_json::from_str(raw).ok()?;
    let mut safe = Map::new();
    for (key, field) in [
        ("hook_event_name", fields.hook_event_name),
        ("session_id", fields.session_id),
        ("agent_id", fields.agent_id),
        ("model", fields.model),
        ("from_model", fields.from_model),
        ("to_model", fields.to_model),
        ("tool_name", fields.tool_name),
    ] {
        // Even fully escaped ASCII identifiers fit in 6 * 256 + two quotes.
        // Do not allocate arbitrary strings or objects supplied as metadata.
        let Some(field) = field.filter(|field| field.get().len() <= 6 * 256 + 2) else {
            continue;
        };
        if let Ok(value) = serde_json::from_str::<String>(field.get())
            && valid_identifier(&value)
        {
            safe.insert(key.into(), Value::String(value));
        }
    }
    Some(Value::Object(safe))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn large_envelopes_keep_only_allowlisted_metadata_in_any_field_order() {
        let private =
            json!({"nested":["PRIVATE".repeat(20_000)], "arguments":{"secret":"PRIVATE"}});
        for raw in [
            format!(
                r#"{{"tool_response":{private},"hook_event_name":"PostToolUse","model":"model-a","tool_name":"Read"}}"#
            ),
            format!(
                r#"{{"hook_event_name":"PostToolUse","model":"model-a","tool_name":"Read","tool_response":{private}}}"#
            ),
        ] {
            assert!(raw.len() > 64 * 1024);
            assert_eq!(
                metadata(raw.as_bytes()).unwrap(),
                json!({"hook_event_name":"PostToolUse", "model":"model-a", "tool_name":"Read"})
            );
        }
    }

    #[test]
    fn limit_covers_the_whole_envelope_including_trailing_whitespace() {
        let mut raw = br#"{"hook_event_name":"Stop"}"#.to_vec();
        raw.resize(INPUT_LIMIT as usize, b' ');
        assert!(metadata(raw.as_slice()).is_some());
        raw.push(b' ');
        assert!(metadata(raw.as_slice()).is_none());
        struct Counted<'a>(&'a mut usize);
        impl Read for Counted<'_> {
            fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
                out.fill(b' ');
                *self.0 += out.len();
                Ok(out.len())
            }
        }
        let mut read = 0;
        assert!(metadata(Counted(&mut read)).is_none());
        assert_eq!(read as u64, INPUT_LIMIT + 1);
    }

    #[test]
    fn malformed_envelopes_are_not_partially_accepted() {
        for raw in [
            br#"{"hook_event_name":"Stop","tool_response":[]} trailing"#.as_slice(),
            br#"{"hook_event_name":"Stop","tool_response":{"broken":}}"#,
            br#"{"hook_event_name":"Stop","hook_event_name":"PostToolUse"}"#,
            br#"{"hook_event_name":"Stop","model":"a","model":"b"}"#,
            br#"[{"hook_event_name":"Stop"}]"#,
            b"{\"hook_event_name\":\"Stop\",\"ignored\":\"\xff\"}",
        ] {
            assert!(metadata(raw).is_none());
        }
    }

    #[test]
    fn invalid_optional_fields_are_omitted_without_allocating_their_json_trees() {
        for value in [
            json!(["PRIVATE".repeat(20_000)]),
            json!(false),
            json!(42),
            json!(null),
            json!("x".repeat(257)),
        ] {
            let raw = json!({"hook_event_name":"Stop", "model":value}).to_string();
            assert_eq!(
                metadata(raw.as_bytes()).unwrap(),
                json!({"hook_event_name":"Stop"})
            );
        }
        let escaped = r#"{"hook_event_name":"Stop","model":"\u006d\u006f\u0064\u0065\u006c"}"#;
        assert_eq!(metadata(escaped.as_bytes()).unwrap()["model"], "model");
    }
}
