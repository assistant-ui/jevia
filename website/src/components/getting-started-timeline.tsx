import { CommandBlock } from "./command-block";

export interface GettingStartedStep {
  number: string;
  label: string;
  title: string;
  description: string;
  command: string;
}

interface GettingStartedTimelineProps {
  steps: GettingStartedStep[];
}

export function GettingStartedTimeline({ steps }: GettingStartedTimelineProps) {
  return (
    <div className="timescale" aria-label="Jevia setup timeline">
      <div className="timescale-header" aria-hidden="true">
        <span>Step</span>
        <span>Setup sequence</span>
      </div>
      <div className="timescale-viewport" tabIndex={0} aria-label="Scroll through setup steps">
        <ol className="timescale-track">
          {steps.map((step) => (
            <li className="timescale-item" key={step.number}>
              <span className="timescale-tick" aria-hidden="true" />
              <div className="timescale-index">
                <span>{step.number}</span>
                <span>{step.label}</span>
              </div>
              <div className="timescale-content">
                <h3>{step.title}</h3>
                <p>{step.description}</p>
                <CommandBlock command={step.command} label={step.label} compact />
              </div>
            </li>
          ))}
        </ol>
      </div>
    </div>
  );
}
