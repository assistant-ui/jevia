import { CommandBlock } from "../components/command-block";
import { HarnessCommand } from "../components/harness-command";
import { InstallCommand } from "../components/install-command";
import { ScrollTimescale } from "../components/scroll-timescale";
import { SiteHeader } from "../components/site-header";
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
      "Run Codex, Claude Code, OpenCode, Gemini CLI, Cursor Agent, Copilot CLI, Aider, Goose, Amp, or any configured command-line agent.",
    command: 'jevia run codex "fix the flaky integration test"',
    animatedHarness: true,
  },
];

export default function HomePage() {
  return (
    <main className="site-shell">
      <SiteHeader />

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

      <footer className="site-footer page-frame" aria-labelledby="node-api-title">
        <div className="node-api-rail">
          <div className="node-api-copy">
            <div>
              <h2 id="node-api-title">Node.js API</h2>
              <p>Route any harness from code and report verified outcomes.</p>
            </div>
          </div>
          <div className="node-api-command">
            <CommandBlock command="npm install jevia" label="Node.js API" compact minimal />
          </div>
          <a
            className="node-api-link"
            href="/docs"
          >
            View API
            <span aria-hidden="true">→</span>
          </a>
        </div>
        <span className="frame-junctions footer-junctions" aria-hidden="true" />
      </footer>
    </main>
  );
}
