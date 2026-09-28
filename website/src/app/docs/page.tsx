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

const STORAGE_SETUP = [
  'const target = { backend: "sqlite", path: ".jevia/jevia.db" } as const;',
  "",
  "// Preview only: no database connection or file changes.",
  "console.log(await jevia.setupStorage(target, { importJsonl: true }));",
  "",
  "// After review, stop all project writers and supervisors before applying.",
  "await jevia.setupStorage(target, {",
  "  apply: true,",
  "  confirmStopped: true,",
  "  importJsonl: true,",
  "});",
  "console.log(await jevia.checkStorage());",
  "console.log(await jevia.checkStorage({ deep: true }));",
].join("\n");

const POSTGRES_SETUP = [
  "// Set JEVIA_DATABASE_URL in your environment before creating the client.",
  "// Never pass the URL as an argument or commit it to config.",
  "const target = {",
  '  backend: "postgres",',
  '  project: "my-app",',
  '  urlEnv: "JEVIA_DATABASE_URL",',
  "} as const;",
  "",
  "console.log(await jevia.setupStorage(target)); // Preview first.",
  "// Apply with the same explicit confirmation and import options above.",
].join("\n");

const ERROR_HANDLING = [
  'import { JeviaClient, JeviaCommandError } from "jevia";',
  "",
  "try {",
  "  await jevia.route(task, { signal: controller.signal });",
  "} catch (error) {",
  "  if (error instanceof JeviaCommandError) {",
  "    console.error(error.message, error.exitCode, error.signal);",
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
  {
    signature: "setupStorage(target, options?)",
    description: "Unreleased: preview opt-in SQLite or PostgreSQL setup; apply only with confirmation.",
  },
  {
    signature: "checkStorage(options?)",
    description: "Unreleased: check the selected storage, or deeply validate it without a write probe.",
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

          <section className="docs-section" id="storage" aria-labelledby="storage-title">
            <div className="docs-section-heading">
              <span>04</span>
              <div>
                <h2 id="storage-title">Opt-in storage</h2>
                <p>Keep JSONL by default. Choose SQLite or PostgreSQL explicitly.</p>
              </div>
            </div>
            <div className="docs-note">
              <strong>Unreleased Node API</strong>
              <p>
                These methods are pending the next npm release and are not in
                <code> jevia@0.1.0</code>. They require CLI 0.1.2 or newer and an
                existing <code>jevia init</code> project. Creating a client never
                connects to a database or changes storage.
              </p>
            </div>
            <CodeBlock code={STORAGE_SETUP} label="SQLite: preview, apply, verify" />
            <p className="docs-body-copy">
              Paths are relative to the project root, or absolute. Applying switches
              the project config last. Nonempty JSONL history requires explicit
              import; the source file stays intact, with no ongoing sync. Routes,
              feedback, and run queries then use the configured database.
            </p>
            <CodeBlock code={POSTGRES_SETUP} label="PostgreSQL: environment-only credentials" />
            <p className="docs-body-copy">
              Provision the database first. <code>urlEnv</code> is an environment
              variable name, not a URL. PostgreSQL requires verified TLS by default;
              <code> allowInsecureLocalhost: true</code> is only for loopback development.
              The project name scopes history, not database permissions.
            </p>
            <div className="docs-note">
              <strong>Explicit operations, inspectable reports</strong>
              <p>
                Both methods return human-readable CLI reports, not stable JSON.
                Normal checks use a rollback-only SQL write probe; deep checks validate
                records without writing or repairing. Neither initializes missing storage.
                Use <code>signal</code> for cancellation and raise the client&apos;s
                <code> timeoutMs</code> for large imports. After failure or cancellation,
                inspect config and destination before retrying: database changes may remain.
                This is not SQL-to-SQL migration or a no-storage mode; cache stays local.
              </p>
            </div>
          </section>

          <section className="docs-section" id="adapters" aria-labelledby="adapters-title">
            <div className="docs-section-heading">
              <span>05</span>
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
              <span>06</span>
              <div>
                <h2 id="errors-title">Cancellation and errors</h2>
                <p>Bound each CLI call and keep process failures inspectable.</p>
              </div>
            </div>
            <CodeBlock code={ERROR_HANDLING} label="Error handling" />
            <div className="docs-note">
              <strong>Structured failures</strong>
              <p>
                Log only the message, exit code, and signal. Raw command, stdout, and
                stderr can contain sensitive data and are for private debugging only.
                Invalid JSON or records raise <code>JeviaProtocolError</code>.
              </p>
            </div>
          </section>

        </article>

        <footer className="docs-footer">
          <div className="docs-footer-spacer" aria-hidden="true" />
          <div className="docs-footer-content">
            <span>Need implementation details?</span>
            <a
              href="https://github.com/assistant-ui/jevia/tree/main/packages/jevia-node"
              target="_blank"
              rel="noreferrer"
            >
              View the package source
              <ArrowUpRight size={14} strokeWidth={1.7} aria-hidden="true" />
            </a>
          </div>
          <span className="frame-junctions docs-footer-junctions" aria-hidden="true" />
          <span className="docs-footer-sidebar-junction" aria-hidden="true" />
        </footer>
      </div>
    </main>
  );
}
