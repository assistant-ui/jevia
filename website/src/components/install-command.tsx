"use client";

import { useSyncExternalStore } from "react";

import { CommandBlock } from "./command-block";

// The origin cannot change without a new page load, so no listener is needed.
const subscribe = () => () => {};
const getOrigin = () => window.location.origin;
const getServerOrigin = () => null;

export function InstallCommand({ compact = false }: { compact?: boolean }) {
  const origin = useSyncExternalStore(subscribe, getOrigin, getServerOrigin);

  return (
    <>
      <CommandBlock
        label={compact ? "Install" : "Install Jevia"}
        command={origin ? `curl -fsSL ${origin}/install.sh | sh` : "Loading install command…"}
        compact={compact}
        minimal
        disabled={!origin}
      />
      <noscript>
        <a href="/install.sh">Download the install script</a>
      </noscript>
    </>
  );
}
