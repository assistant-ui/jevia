use jevia_core::{MAX_SAFE_INTEGER, RouteRecord};
use serde_json::{Value, json};

#[test]
fn history_numbers_match_the_sdk_safe_integer_contract() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/numeric-record.json")).unwrap();
    for schema in 1..=6 {
        for path in [
            "/created_at_ms",
            "/lifecycle/started_at_ms",
            "/lifecycle/finished_at_ms",
            "/outcome_evidence/recorded_at_ms",
            "/feedback/0/recorded_at_ms",
            "/execution/duration_ms",
            "/execution/verification/duration_ms",
            "/execution/observations/events/0/recorded_at_ms",
        ] {
            for value in [
                json!(0),
                json!(MAX_SAFE_INTEGER),
                json!(MAX_SAFE_INTEGER + 1),
                json!(u64::MAX),
                json!(-1),
                json!(0.5),
            ] {
                let expected = value
                    .as_u64()
                    .is_some_and(|number| number <= MAX_SAFE_INTEGER);
                let mut input = fixture.clone();
                input["schema_version"] = json!(schema);
                *input.pointer_mut(path).unwrap() = value.clone();
                let accepted = serde_json::from_value::<RouteRecord>(input)
                    .is_ok_and(|record| record.validate().is_ok());
                assert_eq!(
                    accepted, expected,
                    "schema={schema} path={path} value={value}"
                );
            }
        }
    }
}

#[test]
fn typed_records_and_decisions_are_validated_without_rounding() {
    let mut record: RouteRecord =
        serde_json::from_str(include_str!("fixtures/numeric-record.json")).unwrap();
    record.decision.created_at_ms = MAX_SAFE_INTEGER + 2;
    assert!(record.decision.validate().is_err());
    assert!(record.validate().is_err());
    assert_eq!(record.decision.created_at_ms, 9_007_199_254_740_993);
    record.decision.created_at_ms = 0;
    record
        .execution
        .as_mut()
        .unwrap()
        .observations
        .as_mut()
        .unwrap()
        .events[0]
        .recorded_at_ms = u64::MAX;
    assert!(record.validate().is_err());
}
