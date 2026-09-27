import type { Metadata } from "@farm.js/core";
import { ArrowUpRight } from "lucide-react";

import { CodeBlock } from "../../components/code-block";
import { CommandBlock } from "../../components/command-block";
import { DocsSidebar } from "../../components/docs-sidebar";
import { SiteHeader } from "../../components/site-header";

export const dynamic = "force-static";
export const metadata: Metadata = {
  title: "Node.js API documentation — Jevia",
  description:
    "Use Jevia's Node.js API to route tasks, inspect run records, and report verified outcomes.",
  alternates: { canonical: "/docs" },
  openGraph: {
    title: "Node.js API documentation — Jevia",
    description:
      "Access Jevia routing, run records, feedback, and harness adapters from Node.js.",
    url: "/docs",
    type: "website",
  },
};

const QUICKSTART = [
  'import { JeviaClient } from "jevia";',
  "",
  "const jevia = new JeviaClient({ cwd: process.cwd() });",
  'const route = await jevia.route("fix the flaky integration test");',
  "",
  "console.log(route.tier, route.confidence, route.run_id);",
].join("\n");

const HARNESS_ADAPTER = [
  "const models: Record<string, string> = {",
  '  fast: "provider/small",',
  '  balanced: "provider/standard",',
  '  strong: "provider/frontier",',
  "};",
  "",
  "const result = await runYourHarness({",
  "  task,",
  "  model: models[route.tier],",
  "  runId: route.run_id,",
  "});",
  "",
  "const verified = await verifyResult(result);",
  "await jevia.feedback(",
  "  route.run_id,",
  '  verified ? "success" : "failure",',
  ");",
].join("\n");

const ERROR_HANDLING = [
  'import { JeviaClient, JeviaCommandError } from "jevia";',
  "",
  "try {",
  "  await jevia.route(task, { signal: controller.signal });",
  "} catch (error) {",
  "  if (error instanceof JeviaCommandError) {",
  "    console.error(error.exitCode, error.stderr);",
  "  }",
  "}",
].join("\n");

const METHODS = [
  {
    signature: "version(options?)",
    description: "Return the installed Jevia CLI version after validating its output.",
  },
  {
    signature: "route(task, options?)",
    description: "Choose a capability tier and return the complete typed route record.",
  },
  {
    signature: "feedback(runId, outcome, options?)",
    description: "Record an explicit success, failure, or unknown outcome for a run.",
  },
  {
    signature: "runs(options?)",
    description: "List recent route records with a configurable positive result limit.",
  },
  {
    signature: "show(runId, options?)",
    description: "Read one complete route record, including lifecycle and outcome evidence.",
  },
] as const;

export default function DocsPage() {
  return (
    <main className="site-shell docs-shell">
      <SiteHeader current="docs" />

      <div className="docs-frame page-frame">
        <DocsSidebar />

        <article className="docs-content">
          <header className="docs-hero" id="overview">
            <p>
              Route tasks, inspect run records, and report verified outcomes from
              Node.js while keeping Jevia&apos;s local policy, storage, and learning
              behavior in one place.
            </p>
            <div className="docs-capabilities" aria-label="Package capabilities">
              <span>Typed records</span>
              <span>Shell-free execution</span>
              <span>AbortSignal support</span>
            </div>
          </header>

          <section className="docs-section" id="install" aria-labelledby="install-title">
            <div className="docs-section-heading">
              <span>01</span>
              <div>
                <h2 id="install-title">Install</h2>
                <p>Add the package after installing the Jevia CLI.</p>
              </div>
            </div>
            <CommandBlock command="npm install jevia" label="Install the Node.js API" />
            <div className="docs-note">
              <strong>CLI prerequisite</strong>
              <p>
                The package uses the local <code>jevia</code> executable and never runs an
                installer during <code>npm install</code>. Use the{" "}
                <a href="/#quickstart">landing-page guide</a> to install and configure it.
              </p>
            </div>
          </section>

          <section className="docs-section" id="quickstart" aria-labelledby="quickstart-title">
            <div className="docs-section-heading">
              <span>02</span>
              <div>
                <h2 id="quickstart-title">Create a client and route</h2>
                <p>The client resolves the project config from the working directory.</p>
              </div>
            </div>
            <CodeBlock code={QUICKSTART} label="Quickstart" />
            <p className="docs-body-copy">
              A route returns the selected tier, confidence, probabilities, cache source,
              and a traceable run ID. Routing chooses capability; it does not claim the
              task succeeded.
            </p>
          </section>

          <section className="docs-section" id="methods" aria-labelledby="methods-title">
            <div className="docs-section-heading">
              <span>03</span>
              <div>
                <h2 id="methods-title">Client methods</h2>
                <p>A small API over Jevia&apos;s machine-readable CLI contract.</p>
              </div>
            </div>
            <div className="docs-methods" role="list">
              {METHODS.map((method) => (
                <div className="docs-method" role="listitem" key={method.signature}>
                  <code>{method.signature}</code>
                  <p>{method.description}</p>
                </div>
              ))}
            </div>
          </section>

          <section className="docs-section" id="adapters" aria-labelledby="adapters-title">
            <div className="docs-section-heading">
              <span>04</span>
              <div>
                <h2 id="adapters-title">Open any harness</h2>
                <p>Map Jevia&apos;s tier to the model names your chosen harness accepts.</p>
              </div>
            </div>
            <CodeBlock code={HARNESS_ADAPTER} label="Harness adapter" />
            <p className="docs-body-copy">
              Your adapter can call Codex, Claude Code, OpenCode, Gemini CLI, Cursor
              Agent, Copilot CLI, Aider, Goose, Amp, or a custom runner. Submit feedback
              only after your verifier determines the actual outcome.
            </p>
          </section>

          <section className="docs-section" id="errors" aria-labelledby="errors-title">
            <div className="docs-section-heading">
              <span>05</span>
              <div>
                <h2 id="errors-title">Cancellation and errors</h2>
                <p>Bound each CLI call and keep process failures inspectable.</p>
              </div>
            </div>
            <CodeBlock code={ERROR_HANDLING} label="Error handling" />
            <div className="docs-note">
              <strong>Structured failures</strong>
              <p>
                <code>JeviaCommandError</code> includes the exit code, signal, stdout, and
                stderr. Invalid JSON or records raise <code>JeviaProtocolError</code>.
              </p>
            </div>
          </section>

          <footer className="docs-content-footer">
            <span>Need implementation details?</span>
            <a
              href="https://github.com/assistant-ui/jevia/tree/main/packages/jevia-node"
              target="_blank"
              rel="noreferrer"
            >
              View the package source
              <ArrowUpRight size={14} strokeWidth={1.7} aria-hidden="true" />
            </a>
          </footer>
        </article>
      </div>
    </main>
  );
}
