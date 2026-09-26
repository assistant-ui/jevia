import { ArrowUpRight } from "lucide-react";

import { CommandBlock } from "../components/command-block";
import {
  TimescaleAge,
  TimescaleContent,
  TimescaleHeader,
  TimescaleItem,
  TimescaleRail,
  TimescaleRoot,
  TimescaleTick,
  TimescaleTrack,
  TimescaleViewport,
  TimescaleYear,
} from "../components/timescale";

export const dynamic = "force-static";

const SETUP_STEPS = [
  {
    number: "01",
    label: "Install",
    title: "Install the CLI",
    description: "Build Jevia from the public Rust workspace.",
    command: "cargo install --git https://github.com/assistant-ui/jevia jevia",
  },
  {
    number: "02",
    label: "Initialize",
    title: "Create local policy",
    description: "Add a reviewable .jevia/config.toml to your project.",
    command: "jevia init",
  },
  {
    number: "03",
    label: "Authenticate",
    title: "Set the Jev key",
    description: "Keep the credential in your environment, outside config.",
    command: 'export TYPESAFE_API_KEY="your-key"',
  },
  {
    number: "04",
    label: "Verify",
    title: "Check the full path",
    description: "Validate config, storage, credentials, and one live request.",
    command: "jevia check",
  },
  {
    number: "05",
    label: "Route",
    title: "Route real work",
    description: "Receive a capability tier, confidence, and run identifier.",
    command: 'jevia route "fix the flaky integration test"',
  },
];

export default function HomePage() {
  return (
    <main className="site-shell">
      <section className="hero page-rails" aria-labelledby="hero-title">
        <div className="hero-grid" aria-hidden="true" />

        <div className="hero-topline">
          <span className="wordmark">Jevia</span>
          <a
            className="source-link"
            href="https://github.com/assistant-ui/jevia"
            target="_blank"
            rel="noreferrer"
          >
            GitHub
            <ArrowUpRight size={13} strokeWidth={1.7} aria-hidden="true" />
          </a>
        </div>

        <div className="hero-content">
          <p className="eyebrow">Outcome-aware model routing</p>
          <h1 id="hero-title">
            Route the task.
            <span>Keep the outcome.</span>
          </h1>
          <p className="hero-description">
            A local-first Rust router that learns which model tier works for your coding
            tasks.
          </p>
          <div className="hero-command">
            <CommandBlock
              label="Install Jevia"
              command="cargo install --git https://github.com/assistant-ui/jevia jevia"
            />
          </div>
        </div>
      </section>

      <section className="setup page-rails" aria-labelledby="setup-title">
        <header className="setup-heading">
          <div>
            <p className="section-index">01 / Quickstart</p>
            <h2 id="setup-title">From install to first route.</h2>
          </div>
          <p className="scroll-hint" aria-hidden="true">
            Scroll horizontally <span>→</span>
          </p>
        </header>

        <TimescaleRoot>
          <TimescaleHeader>
            <TimescaleAge>Step</TimescaleAge>
            <TimescaleYear>Action</TimescaleYear>
          </TimescaleHeader>

          <TimescaleViewport
            tabIndex={0}
            aria-label="Jevia setup timeline. Scroll horizontally for all five steps."
          >
            <TimescaleTrack>
              <TimescaleRail />
              {SETUP_STEPS.map((step) => (
                <TimescaleItem key={step.number}>
                  <TimescaleTick />
                  <TimescaleAge>{step.number}</TimescaleAge>
                  <TimescaleYear>{step.label}</TimescaleYear>
                  <TimescaleContent>
                    <h3>{step.title}</h3>
                    <p>{step.description}</p>
                    <CommandBlock command={step.command} label={step.label} compact />
                  </TimescaleContent>
                </TimescaleItem>
              ))}
            </TimescaleTrack>
          </TimescaleViewport>
        </TimescaleRoot>
      </section>
    </main>
  );
}
