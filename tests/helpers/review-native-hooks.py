"""Approve fixture-owned hooks through the real CLI UI, never a trust-file edit.

Only for the isolated, credential-free native-harness test fixture. No prompts
are submitted to a model. Fail closed if the pinned UI or hook count changes.
"""
import fcntl
import os
import pathlib
import pty
import re
import select
import signal
import struct
import subprocess
import sys
import termios
import time

root = pathlib.Path.cwd().resolve()
profile = pathlib.Path(os.environ["CODEX_HOME"]).resolve()
assert root.name.startswith("jevia-native-")
assert profile == root / "isolated-home" / "agent"
assert not (profile / "auth.json").exists(), "Fixture must have no real credentials"
assert "hooks" not in (profile / "config.toml").read_text(), "Review only fresh fixture hooks"

master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
process = subprocess.Popen(
    [sys.argv[1], "harness", "review", "codex", "--launch"],
    stdin=slave, stdout=slave, stderr=slave, start_new_session=True,
    preexec_fn=lambda: fcntl.ioctl(0, termios.TIOCSCTTY, 0),
    env={**os.environ, "TERM": "xterm-256color"},
)
os.close(slave)
buffer = ""
transcript = ""
stage = "startup"
deadline = time.monotonic() + 40
ansi = re.compile(r"\x1b\].*?(?:\x07|\x1b\\)|\x1b\[[0-?]*[ -/]*[@-~]", re.DOTALL)

def key(data):
    # Let the rendered modal become the active input handler before sending a
    # key; native startup can paint it before installing its event reader.
    time.sleep(0.25)
    os.write(master, data)

try:
    while time.monotonic() < deadline and process.poll() is None:
        if not select.select([master], [], [], 0.1)[0]:
            continue
        chunk = os.read(master, 65536)
        transcript = (transcript + chunk.decode("utf-8", errors="replace"))[-30000:]
        if b"\x1b[6n" in chunk:
            os.write(master, b"\x1b[1;1R")
        for code in (b"10", b"11"):
            if b"\x1b]" + code + b";?" in chunk:
                os.write(master, b"\x1b]" + code + b";rgb:0000/0000/0000\x1b\\")
        buffer = (buffer + ansi.sub("", chunk.decode("utf-8", errors="replace")))[-20000:]
        if stage == "startup" and "Trust this folder?" in buffer:
            key(b"\r")
            buffer = ""
        elif stage == "startup" and "8 hooks are new or changed." in buffer and "Review hooks" in buffer:
            key(b"\r")
            buffer = ""
            stage = "review"
        elif stage == "review" and "8 hooks need review" in buffer and "trust all" in buffer:
            key(b"t")
            buffer = ""
            stage = "trusted"
        elif stage == "trusted" and "details" in buffer:
            key(b"\x1b")
            buffer = ""
            stage = "quit"
        elif stage == "quit" and "shortcuts" in buffer:
            key(b"\x03")
            stage = "done"
    assert stage == "done", f"Hook review did not finish at stage {stage}: {ansi.sub('', transcript)[-5000:]}"
    assert process.poll() is not None, f"Review did not exit: {ansi.sub('', transcript)[-3000:]}"
    assert process.returncode == 0, f"Review process failed ({process.returncode}): {ansi.sub('', transcript)[-3000:]}"
    # Check that the native UI persisted all eight decisions. Never manufacture
    # hashes or patch its config/trust database to make the test pass.
    assert (profile / "config.toml").read_text().count("trusted_hash =") == 8
    print("Reviewed eight fixture hooks through the normal native UI")
finally:
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
    os.close(master)
