//! Conservative catalog recognition, not a general SQL equivalence prover.
//! Ignore formatting outside quotes; never erase literal contents, parentheses,
//! operators or casts, and never execute catalog SQL or modify the database.

// pg_get_expr(indpred, indrelid, false) for the predicate created by Jevia.
// PostgreSQL expands IN/NOT IN and adds casts/parentheses during parsing.
// The PostgreSQL contract test ties this fixture to observation_predicate():
// freshly initialized indexes must always be recognized.
// IS JSON: PostgreSQL src/backend/utils/adt/ruleutils.c, T_JsonIsPredicate.
const POSTGRES_CATALOG: &str = r"CASE
    WHEN (NOT (record IS JSON)) THEN true
    ELSE ((json_typeof(((replace(record, '\u0000'::text, '\ufffd'::text))::json -> 'execution'::text)) IS NOT NULL) AND (json_typeof(((replace(record, '\u0000'::text, '\ufffd'::text))::json -> 'execution'::text)) <> 'null'::text) AND (((replace(record, '\u0000'::text, '\ufffd'::text))::json #>> '{lifecycle,state}'::text[]) = ANY (ARRAY['completed'::text, 'launch_failed'::text, 'interrupted'::text, 'cancelled'::text, 'timed_out'::text])) AND ((((replace(record, '\u0000'::text, '\ufffd'::text))::json ->> 'outcome'::text) = 'unknown'::text) OR (COALESCE(((replace(record, '\u0000'::text, '\ufffd'::text))::json #>> '{outcome_evidence,source}'::text[]), ''::text) <> ALL (ARRAY['manual'::text, 'verification'::text]))))
END";

pub(super) fn postgres_matches(expression: &str) -> bool {
    matches!((tokens(expression), tokens(POSTGRES_CATALOG)), (Some(actual), Some(expected)) if actual == expected)
}

pub(super) fn sqlite_matches(definition: &str, expected: &str) -> bool {
    let (Some(mut definition), Some(expected)) = (tokens(definition), tokens(expected)) else {
        return false;
    };
    if definition.last().is_some_and(|token| token == ";") {
        definition.pop();
    }
    // Quoted identifiers/literals remain quoted tokens, so embedded WHERE
    // cannot be mistaken for the predicate boundary.
    definition
        .iter()
        .position(|token| token == "where")
        .is_some_and(|index| definition[index + 1..] == expected)
}

fn tokens(sql: &str) -> Option<Vec<String>> {
    let mut chars = sql.chars().peekable();
    let mut result = Vec::new();
    while let Some(ch) = chars.next() {
        if ch.is_ascii_whitespace() {
            continue;
        }
        let mut token = String::from(ch);
        if matches!(ch, '\'' | '"' | '`') {
            loop {
                let next = chars.next()?;
                token.push(next);
                if next == ch {
                    if chars.peek() == Some(&ch) {
                        token.push(chars.next()?);
                    } else {
                        break;
                    }
                }
            }
        } else if ch.is_ascii_alphanumeric() || ch == '_' {
            while chars
                .peek()
                .is_some_and(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
            {
                token.push(chars.next()?);
            }
            token.make_ascii_lowercase();
        } else if !ch.is_ascii()
            || (ch == '-' && chars.peek() == Some(&'-'))
            || (ch == '/' && chars.peek() == Some(&'*'))
        {
            return None;
        }
        result.push(token);
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formatting_is_ignored_but_literals_and_boolean_structure_are_not() {
        assert!(postgres_matches(&POSTGRES_CATALOG.replace('\n', " ")));
        assert!(!postgres_matches(
            &POSTGRES_CATALOG.replace("'unknown'", "'Unknown'")
        ));
        assert!(!postgres_matches(
            &POSTGRES_CATALOG.replace("'manual'", "' manual'")
        ));
        assert!(!postgres_matches(&POSTGRES_CATALOG.replace("AND", "OR")));
        assert!(!postgres_matches("false"));
        assert!(sqlite_matches(
            "CREATE INDEX \"where\" ON t(x) WHERE x = 'a b';",
            "x='a b'"
        ));
        assert!(!sqlite_matches(
            "CREATE INDEX i ON t(x) WHERE x = 'ab'",
            "x='a b'"
        ));
        assert!(!sqlite_matches(
            "CREATE INDEX i ON t(x) WHERE x='a''b'",
            "x='ab'"
        ));
        assert!(!sqlite_matches(
            "CREATE INDEX i ON t(x) WHERE (x OR y) AND z",
            "x OR (y AND z)"
        ));
        assert!(!sqlite_matches(
            "CREATE INDEX i ON t(x) WHERE x='unterminated",
            "x=1"
        ));
        assert!(!sqlite_matches(
            "CREATE INDEX i ON t(x) WHERE x=1 -- comment",
            "x=1"
        ));
    }
}
