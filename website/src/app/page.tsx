import { ArrowUpRight } from "lucide-react";

import { CommandBlock } from "../components/command-block";
import { HarnessCommand } from "../components/harness-command";
import { InstallCommand } from "../components/install-command";
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
    description: "Install a verified prebuilt binary. No Rust or Cargo required.",
    command: null,
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
    title: "Preview the route",
    description: "Receive a model tier, confidence, and traceable run ID.",
    command: 'jevia route "fix the flaky integration test"',
  },
  {
    number: "06",
    label: "Launch",
    title: "Open any harness",
    description:
      "Run a configured Codex, Claude Code, OpenCode, Gemini CLI, or any other command-line agent. Jevia selects its model and records the outcome.",
    command: 'jevia run codex "fix the flaky integration test"',
    animatedHarness: true,
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
          <a href="https://github.com/assistant-ui/jevia" target="_blank" rel="noreferrer">
            {/* GitHub's Octicons mark, MIT; see public/third-party-notices.txt. */}
            <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
              <path d="M6.766 11.328c-2.063-.25-3.516-1.734-3.516-3.656 0-.781.281-1.625.75-2.188-.203-.515-.172-1.609.063-2.062.625-.078 1.468.25 1.968.703.594-.187 1.219-.281 1.985-.281.765 0 1.39.094 1.953.265.484-.437 1.344-.765 1.969-.687.218.422.25 1.515.046 2.047.5.593.766 1.39.766 2.203 0 1.922-1.453 3.375-3.547 3.64.531.344.89 1.094.89 1.954v1.625c0 .468.391.734.86.547C13.781 14.359 16 11.53 16 8.03 16 3.61 12.406 0 7.984 0 3.563 0 0 3.61 0 8.031a7.88 7.88 0 0 0 5.172 7.422c.422.156.828-.125.828-.547v-1.25c-.219.094-.5.156-.75.156-1.031 0-1.64-.562-2.078-1.609-.172-.422-.36-.672-.719-.719-.187-.015-.25-.093-.25-.187 0-.188.313-.328.625-.328.453 0 .844.281 1.25.86.313.452.64.655 1.031.655s.641-.14 1-.5c.266-.265.47-.5.657-.656" />
            </svg>
            GitHub
            <ArrowUpRight size={13} strokeWidth={1.7} aria-hidden="true" />
          </a>
        </nav>
      </header>

      <section id="top" className="hero page-frame" aria-labelledby="hero-title">
        <div className="hero-copy">
          <h1 id="hero-title">
            Model routing that <span className="hero-highlight">learns.</span>
          </h1>
          <p className="hero-description">
            Jevia routes your task to a model tier and uses verified outcomes to improve
            future decisions.
          </p>
          <div className="hero-command">
            <InstallCommand />
          </div>
        </div>
        <span className="frame-junctions" aria-hidden="true" />
      </section>

      <ScrollTimescale>
        <TimescaleRoot className="setup-timescale">
          <TimescaleViewport
            tabIndex={0}
            role="region"
            aria-label="Jevia setup timeline. Scroll up or down, swipe horizontally, or use the arrow keys to explore six steps."
          >
            <TimescaleTrack role="list">
              <TimescaleRail />

              {SETUP_STEPS.map((step) => (
                <TimescaleItem key={step.number} role="listitem">
                  <TimescaleTick />
                  <TimescaleContent>
                    <span className="timescale-step-number" aria-hidden="true">
                      {step.number}
                    </span>
                    <h3>{step.title}</h3>
                    <p>{step.description}</p>
                    {step.animatedHarness ? (
                      <HarnessCommand />
                    ) : step.command === null ? (
                      <InstallCommand compact />
                    ) : (
                      <CommandBlock command={step.command} label={step.label} compact minimal />
                    )}
                  </TimescaleContent>
                </TimescaleItem>
              ))}
            </TimescaleTrack>
          </TimescaleViewport>
        </TimescaleRoot>
      </ScrollTimescale>

      <footer className="site-footer page-frame" aria-hidden="true">
        <span className="frame-junctions footer-junctions" />
      </footer>
    </main>
  );
}
