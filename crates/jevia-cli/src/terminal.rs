//! Escape display-only metadata, never stored values, subprocess arguments, or JSON.
use jevia_core::RouteRecord;

pub fn text(value: &str) -> std::str::EscapeDebug<'_> {
    value.escape_debug()
}

pub fn route(record: &RouteRecord) -> String {
    let decision = &record.decision;
    format!(
        "run:        {}\ntier:       {}\nsuggested:  {}\nconfidence: {:.2}\nfallback:   {}\njev model:  {}\nsource:     {}\n",
        text(&decision.run_id),
        text(&decision.tier),
        text(&decision.suggested_tier),
        decision.confidence,
        decision.fallback_applied,
        text(&decision.jev_model),
        decision.source,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn metadata_cannot_emit_controls_or_spoof_additional_lines() {
        let hostile = "value\u{1b}[31m\r\n\t\u{7}\u{8}\u{9b}\u{202e}\0";
        let record: RouteRecord = serde_json::from_value(json!({
            "schema_version": 6, "run_id": hostile, "tier": hostile, "suggested_tier": hostile,
            "confidence": 0.9, "probabilities": {}, "fallback_applied": false,
            "jev_model": hostile, "created_at_ms": 1, "task": null, "outcome": "unknown"
        }))
        .unwrap();
        let rendered = route(&record);
        assert_eq!(rendered.lines().count(), 7);
        assert!(!rendered.chars().any(|c| c.is_control() && c != '\n'));
        assert!(!rendered.contains('\u{202e}'));
        assert!(rendered.contains("\\u{1b}[31m\\r\\n\\t"));
        assert_eq!(record.decision.run_id, hostile);
        assert_eq!(
            text("provider/model-v2 模型 🦀").to_string(),
            "provider/model-v2 模型 🦀"
        );
    }
}
