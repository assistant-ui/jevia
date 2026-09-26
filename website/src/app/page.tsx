import { ArrowUpRight } from "lucide-react";

import { CommandBlock } from "../components/command-block";
import { ScrollTimescale } from "../components/scroll-timescale";
import {
  TimescaleContent,
  TimescaleItem,
  TimescaleRail,
  TimescaleRoot,
  TimescaleTick,
  TimescaleTrack,
  TimescaleViewport,
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

export default function HomePage() {
  return (
    <main className="site-shell">
      <header className="site-header page-frame">
        <a className="wordmark" href="#top" aria-label="Jevia home">
          <span className="wordmark-mark" aria-hidden="true">
            J~
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
        <div className="hero-copy">
          <h1 id="hero-title">Model routing that learns.</h1>
          <p className="hero-description">
            Jevia routes your task to a model tier and uses verified outcomes to improve
            future decisions.
          </p>
          <div className="hero-command">
            <CommandBlock
              label="Install Jevia"
              command="cargo install --git https://github.com/assistant-ui/jevia jevia"
              minimal
            />
          </div>
        </div>
        <span className="frame-junctions" aria-hidden="true" />
      </section>

      <ScrollTimescale>
        <TimescaleRoot className="setup-timescale">
          <TimescaleViewport
            tabIndex={0}
            role="region"
            aria-label="Jevia setup timeline. Scroll down, swipe horizontally, or use the arrow keys to explore five steps."
          >
            <TimescaleTrack role="list">
              <TimescaleRail />

              {SETUP_STEPS.map((step) => (
                <TimescaleItem key={step.number} role="listitem">
                  <TimescaleTick />
                  <TimescaleContent>
                    <h3>{step.title}</h3>
                    <p>{step.description}</p>
                    <CommandBlock command={step.command} label={step.label} compact minimal />
                  </TimescaleContent>
                </TimescaleItem>
              ))}
            </TimescaleTrack>
          </TimescaleViewport>
        </TimescaleRoot>
      </ScrollTimescale>
    </main>
  );
}
