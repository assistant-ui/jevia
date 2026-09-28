const args = process.argv.slice(2);

function record(overrides = {}) {
  return {
    schema_version: 3,
    run_id: "run-test",
    tier: "balanced",
    suggested_tier: "balanced",
    confidence: 0.86,
    probabilities: { fast: 0.04, balanced: 0.86, strong: 0.1 },
    fallback_applied: false,
    jev_model: "jev-test",
    created_at_ms: 1,
    source: "live",
    task: "test task",
    outcome: "unknown",
    lifecycle: { state: "routed", started_at_ms: null, finished_at_ms: null },
    ...overrides,
  };
}

if (args[0] === "--version") {
  console.log("jevia 0.2.0");
} else if (args[0] === "route") {
  if (args[1] === "fail") {
    console.error("error: simulated routing failure");
    process.exitCode = 2;
  } else if (args[1] === "wait") {
    await new Promise((resolve) => setTimeout(resolve, 1_000));
    console.log(JSON.stringify(record({ task: args[1] })));
  } else {
    console.log(
      JSON.stringify(
        record({ task: args[1], jev_model: args.join("|") }),
      ),
    );
  }
} else if (args[0] === "feedback") {
  const reasonIndex = args.indexOf("--reason");
  console.log(
    JSON.stringify(
      record({
        run_id: args[1],
        outcome: args[2],
        outcome_evidence: { source: "manual", recorded_at_ms: 2 },
        feedback: [
          {
            previous_outcome: "unknown",
            previous_source: null,
            outcome: args[2],
            recorded_at_ms: 2,
            reason: reasonIndex === -1 ? null : args[reasonIndex + 1],
          },
        ],
      }),
    ),
  );
} else if (args[0] === "runs" && args[1] === "show") {
  console.log(JSON.stringify(record({ run_id: args[2] })));
} else if (args[0] === "runs") {
  const limitIndex = args.indexOf("--limit");
  const limit = Number(args[limitIndex + 1]);
  console.log(
    JSON.stringify(
      Array.from({ length: Math.min(limit, 2) }, (_, index) =>
        record({ run_id: `run-${index + 1}` }),
      ),
    ),
  );
} else {
  console.error(`error: unexpected arguments: ${args.join(" ")}`);
  process.exitCode = 2;
}
