import { ArrowRight, Check, RotateCcw } from "lucide-react";

const RUN_STAGES = [
  {
    number: "01",
    label: "Route",
    value: "strong tier",
    detail: "confidence 0.86",
  },
  {
    number: "02",
    label: "Run",
    value: "claude",
    detail: "configured model",
  },
  {
    number: "03",
    label: "Verify",
    value: "cargo test --workspace",
    detail: "exit 0",
  },
  {
    number: "04",
    label: "Record",
    value: "success",
    detail: "learning evidence",
  },
] as const;

export function LearningLoop() {
  return (
    <section className="learning-loop page-frame" aria-labelledby="learning-loop-title">
      <span
        className="frame-junctions section-junctions section-junctions-top"
        aria-hidden="true"
      />

      <div className="learning-loop-grid">
        <header className="learning-loop-copy">
          <p className="section-kicker">One run, end to end</p>
          <h2 id="learning-loop-title">From task to evidence.</h2>
          <p className="learning-loop-description">
            Jevia routes the task, launches your chosen harness, verifies the result,
            and carries trusted outcomes into future decisions.
          </p>

          <div className="learning-loop-sequence" aria-label="Route, run, verify, learn">
            <span>Route</span>
            <ArrowRight size={13} strokeWidth={1.6} aria-hidden="true" />
            <span>Run</span>
            <ArrowRight size={13} strokeWidth={1.6} aria-hidden="true" />
            <span>Verify</span>
            <ArrowRight size={13} strokeWidth={1.6} aria-hidden="true" />
            <span>Learn</span>
            <RotateCcw size={13} strokeWidth={1.6} aria-hidden="true" />
          </div>
        </header>

        <div className="run-trace">
          <div className="run-trace-header">
            <span>Example run</span>
            <span className="run-trace-status">
              <Check size={12} strokeWidth={2} aria-hidden="true" />
              Completed
            </span>
          </div>

          <div className="run-trace-task">
            <span>Task</span>
            <code>fix the flaky integration test</code>
          </div>

          <ol className="run-trace-stages">
            {RUN_STAGES.map((stage) => (
              <li key={stage.number}>
                <span className="run-stage-number" aria-hidden="true">
                  {stage.number}
                </span>
                <div className="run-stage-main">
                  <span>{stage.label}</span>
                  <code>{stage.value}</code>
                </div>
                <span className="run-stage-detail">{stage.detail}</span>
              </li>
            ))}
          </ol>

          <div className="run-trace-note">
            <span>Learning rule</span>
            <p>
              Verifier-backed results and explicit feedback become learning evidence.
              Process exits remain visible in history, but are not treated as proof.
            </p>
          </div>
        </div>
      </div>

      <span
        className="frame-junctions section-junctions section-junctions-bottom"
        aria-hidden="true"
      />
    </section>
  );
}
