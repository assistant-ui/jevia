//! Escape display-only metadata, never stored values, subprocess arguments, or JSON.
use jevia_core::RouteRecord;

pub fn text(value: &str) -> std::str::EscapeDebug<'_> {
    value.escape_debug()
}

/// One-line application diagnostics, including nested error chains. Preserve
/// quotes/backslashes so ordinary paths and already-escaped metadata stay readable.
pub fn diagnostic(value: impl std::fmt::Display) -> String {
    let mut output = String::new();
    for c in value.to_string().chars() {
        if matches!(c, '\\' | '\'' | '"') {
            output.push(c);
        } else {
            output.extend(c.escape_debug());
        }
    }
    output
}

pub fn path(value: &std::path::Path) -> String {
    diagnostic(value.display())
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
    fn error_chains_are_one_line_without_double_escaping_safe_text() {
        let error = anyhow::anyhow!("inner\u{1b}[31m\r\nspoofed\u{202e}").context("outer\tcontext");
        let result = diagnostic(format_args!("{error:#}"));
        assert!(!result.chars().any(char::is_control));
        assert!(!result.contains('\u{202e}'));
        assert!(result.contains("outer\\tcontext: inner\\u{1b}[31m\\r\\nspoofed"));
        let safe = r#"C:\work\模型 🦀\file 'quoted' \u{1b}"#;
        assert_eq!(diagnostic(safe), safe);
    }

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
