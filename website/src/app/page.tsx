import { ArrowRight, ArrowUpRight } from "lucide-react";

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
    description: "Build the Jevia binary from the public Rust workspace.",
    command: "cargo install --git https://github.com/assistant-ui/jevia jevia",
  },
  {
    number: "02",
    label: "Initialize",
    title: "Create local policy",
    description: "Add a reviewable .jevia/config.toml to this project.",
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
    description: "Receive a model tier, confidence, and traceable run ID.",
    command: 'jevia route "fix the flaky integration test"',
  },
];

const FOOTER_DETAILS = [
  { label: "Runtime", value: "Rust" },
  { label: "Router", value: "Jev" },
  { label: "Mode", value: "Local-first" },
  { label: "Cache", value: "Exact + bounded" },
  { label: "Source code", value: "GitHub", href: "https://github.com/assistant-ui/jevia" },
  {
    label: "License",
    value: "MIT License",
    href: "https://github.com/assistant-ui/jevia/blob/main/LICENSE",
  },
  { label: "Typeface", value: "Geist" },
  { label: "Interface", value: "Farm.js + React" },
];

export default function HomePage() {
  return (
    <main className="site-shell">
      <header className="site-header page-frame">
        <a className="wordmark" href="#top" aria-label="Jevia home">
          <span className="wordmark-mark" aria-hidden="true">
            J
          </span>
          <span>Jevia</span>
        </a>

        <nav className="site-nav" aria-label="Primary navigation">
          <a href="#quickstart">Quickstart</a>
          <a href="https://github.com/assistant-ui/jevia" target="_blank" rel="noreferrer">
            GitHub
            <ArrowUpRight size={13} strokeWidth={1.7} aria-hidden="true" />
          </a>
        </nav>
      </header>

      <section id="top" className="hero page-frame" aria-labelledby="hero-title">
        <p className="eyebrow">Local-first / outcome-aware / Rust</p>
        <h1 id="hero-title">
          Route the task.
          <span>Learn from the result.</span>
        </h1>
        <p className="hero-description">
          Jevia picks a model tier, records the verified outcome, and makes the next route
          with better evidence.
        </p>

        <div className="hero-command">
          <CommandBlock
            label="Install Jevia"
            command="cargo install --git https://github.com/assistant-ui/jevia jevia"
          />
        </div>

        <a className="text-link" href="#quickstart">
          View the setup path
          <ArrowRight size={14} strokeWidth={1.7} aria-hidden="true" />
        </a>
      </section>

      <section id="quickstart" className="quickstart page-frame" aria-labelledby="setup-title">
        <header className="section-heading">
          <div>
            <p className="section-index">Quickstart / 01—05</p>
            <h2 id="setup-title">From install to first route.</h2>
          </div>
          <p className="scroll-hint" aria-hidden="true">
            Scroll horizontally <span>→</span>
          </p>
        </header>

        <TimescaleRoot className="setup-timescale">
          <TimescaleHeader>
            <TimescaleAge>Step</TimescaleAge>
            <TimescaleYear>Action</TimescaleYear>
          </TimescaleHeader>

          <TimescaleViewport
            tabIndex={0}
            aria-label="Jevia setup timeline. Scroll horizontally for all five steps."
          >
            <TimescaleTrack role="list">
              <TimescaleRail />

              {SETUP_STEPS.map((step) => (
                <TimescaleItem key={step.number} role="listitem">
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

      <footer className="site-footer page-frame">
        <div className="footer-intro">
          <strong>jevia</strong>
          <p>Outcome-aware model routing for coding agents and local harnesses.</p>
        </div>

        <dl className="footer-grid">
          {FOOTER_DETAILS.map((detail) => (
            <div className="footer-cell" key={detail.label}>
              <dt>{detail.label}</dt>
              <dd>
                {detail.href ? (
                  <a href={detail.href} target="_blank" rel="noreferrer">
                    {detail.value}
                  </a>
                ) : (
                  detail.value
                )}
              </dd>
            </div>
          ))}
        </dl>

        <div className="footer-built-for">
          <span>Built for</span>
          <ol>
            <li>
              <span>01</span> Coding agents
            </li>
            <li>
              <span>02</span> CLI harnesses
            </li>
            <li>
              <span>03</span> Local workflows
            </li>
            <li>
              <span>04</span> Verifiable routing
            </li>
          </ol>
        </div>

        <div className="footer-bottom">
          <span className="wordmark-mark" aria-hidden="true">
            J
          </span>
          <span>Jevia · Open source</span>
          <a href="https://github.com/assistant-ui/jevia" target="_blank" rel="noreferrer">
            Source
          </a>
        </div>
      </footer>
    </main>
  );
}
