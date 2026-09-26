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
    description: "Build the Jevia binary from the public Rust workspace.",
    command: "cargo install --git https://github.com/assistant-ui/jevia jevia",
    scope: "PKG",
  },
  {
    number: "02",
    label: "Initialize",
    title: "Create local policy",
    description: "Add a reviewable .jevia/config.toml to this project.",
    command: "jevia init",
    scope: "CFG",
  },
  {
    number: "03",
    label: "Authenticate",
    title: "Set the Jev key",
    description: "Keep the credential in your environment, outside config.",
    command: 'export TYPESAFE_API_KEY="your-key"',
    scope: "ENV",
  },
  {
    number: "04",
    label: "Verify",
    title: "Check the full path",
    description: "Validate config, storage, credentials, and one live request.",
    command: "jevia check",
    scope: "CHK",
  },
  {
    number: "05",
    label: "Route",
    title: "Route real work",
    description: "Receive a model tier, confidence, and traceable run ID.",
    command: 'jevia route "fix the flaky integration test"',
    scope: "RUN",
  },
];

export default function HomePage() {
  return (
    <main className="site-shell">
      <header className="topbar">
        <a className="wordmark" href="#top" aria-label="Jevia home">
          <span className="brand-mark" aria-hidden="true" />
          <span>Jevia</span>
        </a>

        <span className="topbar-status" aria-label="Jevia is a local-first Rust router">
          <span className="status-dot" aria-hidden="true" />
          Local-first / Rust
        </span>

        <a
          className="source-link"
          href="https://github.com/assistant-ui/jevia"
          target="_blank"
          rel="noreferrer"
        >
          Source
          <ArrowUpRight size={13} strokeWidth={1.7} aria-hidden="true" />
        </a>
      </header>

      <section id="top" className="hero" aria-labelledby="hero-title">
        <div className="hero-wash" aria-hidden="true" />
        <div className="hero-layout">
          <div className="hero-copy">
            <p className="eyebrow">Outcome-aware model routing</p>
            <h1 id="hero-title">
              Route every task
              <span>with context.</span>
            </h1>
          </div>

          <div className="hero-aside">
            <p className="hero-description">
              Jevia picks a model tier, records the verified outcome, and makes the next
              route with better evidence.
            </p>
            <CommandBlock
              label="Install Jevia"
              command="cargo install --git https://github.com/assistant-ui/jevia jevia"
            />
          </div>
        </div>
      </section>

      <section className="console-stage" aria-labelledby="setup-title">
        <div className="router-console">
          <div className="console-toolbar">
            <span className="console-app-mark" aria-hidden="true">
              &gt;_
            </span>
            <div className="console-path">
              <strong>jevia</strong>
              <span>/</span>
              <span>quickstart</span>
            </div>
            <div className="console-connection">
              <span className="status-dot" aria-hidden="true" />
              connected
            </div>
          </div>

          <div className="console-layout">
            <aside className="console-rail" aria-hidden="true">
              <span className="console-rail-active">⌁</span>
              <span>+</span>
              <span>≡</span>
              <span>··</span>
              <i />
            </aside>

            <div className="console-workspace">
              <header className="setup-heading">
                <div>
                  <p className="section-index">SETUP_SEQUENCE / 01—05</p>
                  <h2 id="setup-title">First route, five commands.</h2>
                </div>
                <p className="scroll-hint" aria-hidden="true">
                  [ shift + scroll ] <span>→</span>
                </p>
              </header>

              <TimescaleRoot>
                <TimescaleHeader>
                  <TimescaleAge>LOCAL://SETUP</TimescaleAge>
                  <TimescaleYear>05 STEPS</TimescaleYear>
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
                          <span className="step-scope">[{step.scope}]</span>
                          <h3>{step.title}</h3>
                          <p>{step.description}</p>
                          <CommandBlock command={step.command} label={step.label} compact />
                        </TimescaleContent>
                      </TimescaleItem>
                    ))}
                  </TimescaleTrack>
                </TimescaleViewport>
              </TimescaleRoot>

              <footer className="console-footer" aria-hidden="true">
                <span>MODE: LOCAL</span>
                <span>CACHE: READY</span>
                <span className="console-footer-spacer" />
                <span>HORIZONTAL INPUT ENABLED</span>
              </footer>
            </div>
          </div>
        </div>
      </section>
    </main>
  );
}
