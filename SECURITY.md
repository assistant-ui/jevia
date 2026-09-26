# Security policy

## Reporting a vulnerability

Please do not open a public issue for a suspected vulnerability. Use GitHub's
private vulnerability reporting for this repository so maintainers can assess
the report before details are disclosed.

Include the affected version, reproduction steps, impact, and any suggested
mitigation. Do not include real API keys or sensitive task history.

## Credential handling

Jevia reads the Jev credential from `TYPESAFE_API_KEY`. It must not be stored in
`.jevia/config.toml`, committed to Git, or included in diagnostic output.

Local run history can contain task text. `.jevia/runs.jsonl` is ignored by
default, and users can disable task-text persistence in configuration.
