import { ArrowDown, ArrowRight, ArrowUpRight, CircleCheck } from "lucide-react";

import { CommandBlock } from "../components/command-block";
import {
  GettingStartedTimeline,
  type GettingStartedStep,
} from "../components/getting-started-timeline";
import { SiteHeader } from "../components/site-header";

export const dynamic = "force-static";

const GETTING_STARTED_STEPS: GettingStartedStep[] = [
  {
    number: "01",
    label: "Install",
    title: "Install the CLI",
    description: "Build the current Jevia binary from its public Rust workspace.",
    command: "cargo install --git https://github.com/assistant-ui/jevia jevia",
  },
  {
    number: "02",
    label: "Initialize",
    title: "Create local policy",
    description: "Add a reviewable .jevia/config.toml to the project you want to route.",
    command: "jevia init",
  },
  {
    number: "03",
    label: "Authenticate",
    title: "Set the Jev key",
    description: "Keep the credential in your environment. Jevia never writes it to config.",
    command: 'export TYPESAFE_API_KEY="your-key"',
  },
  {
    number: "04",
    label: "Verify",
    title: "Check the full path",
    description: "Validate config, storage, credentials, and one live Jev round trip.",
    command: "jevia check",
  },
  {
    number: "05",
    label: "Route",
    title: "Route real work",
    description: "Get a capability tier, confidence, and traceable run identifier.",
    command: 'jevia route "fix the flaky integration test"',
  },
];

const LOOP_STEPS = [
  {
    number: "01",
    title: "Route",
    description: "Jev selects the least expensive capability tier likely to succeed.",
  },
  {
    number: "02",
    title: "Verify",
    description: "Your harness and verifier turn execution into success or failure evidence.",
  },
  {
    number: "03",
    title: "Adapt",
    description: "Relevant completed outcomes inform the next routing decision.",
  },
];

export default function HomePage() {
  return (
    <main id="top" className="site-shell">
      <SiteHeader />

      <section className="hero section-rails" aria-labelledby="hero-title">
        <div className="hero-grid" aria-hidden="true" />
        <div className="hero-content">
          <div className="eyebrow">
            <span className="status-dot" />
            Local-first model routing for coding agents
          </div>
          <h1 id="hero-title">
            Route the task.
            <span>Learn from the result.</span>
          </h1>
          <p className="hero-description">
            Jevia asks Jev for a typed model-tier decision, verifies what happened, and uses
            real outcomes to make the next route better.
          </p>

          <div className="hero-command">
            <CommandBlock
              label="Install Jevia"
              command="cargo install --git https://github.com/assistant-ui/jevia jevia"
            />
          </div>

          <div className="hero-actions">
            <a className="primary-link" href="#get-started">
              Get started
              <ArrowDown size={14} strokeWidth={1.8} aria-hidden="true" />
            </a>
            <a
              className="secondary-link"
              href="https://github.com/assistant-ui/jevia"
              target="_blank"
              rel="noreferrer"
            >
              View source
              <ArrowUpRight size={14} strokeWidth={1.8} aria-hidden="true" />
            </a>
          </div>
        </div>

        <div className="hero-footer" aria-label="Product properties">
          <span>Rust CLI</span>
          <span>Local evidence</span>
          <span>Exact cache</span>
          <span>Typed decisions</span>
        </div>
      </section>

      <section id="how-it-works" className="loop-section section-rails" aria-labelledby="loop-title">
        <div className="section-heading">
          <span className="section-index">01 / How it works</span>
          <h2 id="loop-title">A router that remembers what happened.</h2>
          <p>
            Classification is only the start. Jevia closes the loop between a routing
            decision and the verified outcome that followed it.
          </p>
        </div>

        <ol className="loop-grid">
          {LOOP_STEPS.map((step, index) => (
            <li key={step.number}>
              <div className="loop-number">{step.number}</div>
              <h3>{step.title}</h3>
              <p>{step.description}</p>
              {index < LOOP_STEPS.length - 1 ? (
                <ArrowRight className="loop-arrow" size={16} strokeWidth={1.5} aria-hidden="true" />
              ) : null}
            </li>
          ))}
        </ol>
      </section>

      <section id="get-started" className="quickstart-section section-rails" aria-labelledby="quickstart-title">
        <div className="section-heading compact-heading">
          <span className="section-index">02 / Quickstart</span>
          <h2 id="quickstart-title">From install to first route.</h2>
          <p>Five small steps. Every command is copyable, visible, and easy to verify.</p>
        </div>

        <GettingStartedTimeline steps={GETTING_STARTED_STEPS} />
      </section>

      <section id="check" className="check-section section-rails" aria-labelledby="check-title">
        <div className="check-copy">
          <span className="section-index">03 / Live check</span>
          <h2 id="check-title">Know it works before routing real work.</h2>
          <p>
            <code>jevia check</code> validates the local project and completes one live Jev
            request. It does not add a synthetic run to your history.
          </p>

          <ul className="check-list">
            <li>
              <CircleCheck size={15} strokeWidth={1.8} aria-hidden="true" />
              Config and capability tiers are valid
            </li>
            <li>
              <CircleCheck size={15} strokeWidth={1.8} aria-hidden="true" />
              Local history and cache are readable
            </li>
            <li>
              <CircleCheck size={15} strokeWidth={1.8} aria-hidden="true" />
              Credential, network, and response decoding succeed
            </li>
          </ul>
        </div>

        <div className="terminal" aria-label="Example output from jevia check">
          <div className="terminal-header">
            <span>jevia check</span>
            <span className="terminal-status">
              <span className="status-dot" /> ready
            </span>
          </div>
          <div className="terminal-body">
            <div>
              <span className="terminal-prompt">$</span> jevia check
            </div>
            <div className="terminal-gap" />
            <div>
              <span className="terminal-muted">config:</span> ok (.jevia/config.toml)
            </div>
            <div>
              <span className="terminal-muted">tiers:</span> ok (3)
            </div>
            <div>
              <span className="terminal-muted">local store:</span> ok (0 records)
            </div>
            <div>
              <span className="terminal-muted">routing cache:</span> ok (0 active)
            </div>
            <div>
              <span className="terminal-muted">TYPESAFE_API_KEY:</span> set
            </div>
            <div>
              <span className="terminal-muted">jev api:</span> ok (tier=fast, confidence=0.94)
            </div>
            <div className="terminal-gap" />
            <div className="terminal-success">jevia: ready</div>
          </div>
        </div>
      </section>

      <section className="final-cta section-rails" aria-labelledby="final-title">
        <span className="section-index">04 / Start routing</span>
        <h2 id="final-title">Give every task the model it needs.</h2>
        <p>Start locally. Keep the evidence. Change the policy when you choose.</p>
        <div className="final-actions">
          <a className="primary-link light" href="#get-started">
            Copy the setup
            <ArrowRight size={14} strokeWidth={1.8} aria-hidden="true" />
          </a>
          <a
            className="footer-source-link"
            href="https://github.com/assistant-ui/jevia/blob/main/README.md"
            target="_blank"
            rel="noreferrer"
          >
            Read the docs
          </a>
        </div>
      </section>

      <footer className="site-footer section-rails">
        <span>Jevia</span>
        <span>Outcome-aware model routing</span>
        <a href="https://github.com/assistant-ui/jevia" target="_blank" rel="noreferrer">
          GitHub
        </a>
      </footer>
    </main>
  );
}
