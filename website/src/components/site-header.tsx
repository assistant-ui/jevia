import { ArrowUpRight } from "lucide-react";

export function SiteHeader() {
  return (
    <header className="site-header">
      <a className="wordmark" href="#top" aria-label="Jevia home">
        <span className="wordmark-mark" aria-hidden="true">
          J
        </span>
        <span>Jevia</span>
      </a>

      <nav className="site-nav" aria-label="Primary navigation">
        <a href="#how-it-works">How it works</a>
        <a href="#get-started">Get started</a>
        <a href="#check">Check</a>
      </nav>

      <a
        className="github-link"
        href="https://github.com/assistant-ui/jevia"
        target="_blank"
        rel="noreferrer"
      >
        GitHub
        <ArrowUpRight size={13} strokeWidth={1.8} aria-hidden="true" />
      </a>
    </header>
  );
}
