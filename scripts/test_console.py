#!/usr/bin/env python3
"""Real PTY lifecycle + operator smoke checks. Standard library only, macOS/Unix.
Run after cargo build --locked: python3 scripts/test_console.py
The tiny VT reader only interprets sequences emitted by this console; snapshots
are supplemental evidence, not a general terminal emulator or buffer-test oracle.
"""
import codecs
import fcntl
import os
from pathlib import Path
import pty
import re
import select
import signal
import struct
import subprocess
import sys
import termios
import time

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "target" / "console-pty"


class Screen:
    def __init__(self, width, height):
        self.width, self.height = width, height
        self.rows = [[" "] * width for _ in range(height)]
        self.x = self.y = 0
        self.pending = ""
        self.decoder = codecs.getincrementaldecoder("utf8")("replace")

    def feed(self, data):
        self.pending += self.decoder.decode(data)
        while self.pending:
            if self.pending.startswith("\x1b"):
                match = re.match(r"\x1b\[([0-9;?]*)([ -/]*)([@-~])", self.pending)
                if match:
                    raw, _, op = match.groups()
                    args = [int(p or 0) for p in raw.lstrip("?").split(";")]
                    n = args[0] or 1
                    if op in "Hf":
                        self.y = min(self.height - 1, max(0, n - 1))
                        self.x = min(self.width - 1, max(0, (args[1] if len(args) > 1 else 1) - 1))
                    elif op == "G":
                        self.x = min(self.width - 1, n - 1)
                    elif op == "A":
                        self.y = max(0, self.y - n)
                    elif op == "B":
                        self.y = min(self.height - 1, self.y + n)
                    elif op == "C":
                        self.x = min(self.width - 1, self.x + n)
                    elif op == "D":
                        self.x = max(0, self.x - n)
                    elif op == "J" and args[0] == 2:
                        self.rows = [[" "] * self.width for _ in range(self.height)]
                    elif op == "K":
                        start, stop = (0, self.width) if args[0] == 2 else ((0, self.x + 1) if args[0] == 1 else (self.x, self.width))
                        self.rows[self.y][start:stop] = [" "] * (stop - start)
                    self.pending = self.pending[match.end():]
                    continue
                if len(self.pending) < 2 or self.pending.startswith("\x1b["):
                    break
                self.pending = self.pending[2:]
                continue
            c, self.pending = self.pending[0], self.pending[1:]
            if c == "\r":
                self.x = 0
            elif c == "\n":
                self.y = min(self.height - 1, self.y + 1)
            elif c >= " " and self.x < self.width:
                self.rows[self.y][self.x] = c
                self.x += 1

    def text(self):
        return "\n".join("".join(row) for row in self.rows)


class Session:
    def __init__(self, scenario="balanced", width=80, height=18):
        self.master, self.slave = pty.openpty()
        self.before = termios.tcgetattr(self.slave)
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", height, width, 0, 0))
        self.screen = Screen(width, height)
        self.raw = bytearray()
        def setup():
            os.setsid()
            fcntl.ioctl(self.slave, termios.TIOCSCTTY, 0)
        self.process = subprocess.Popen(
            [sys.executable, "-u", "-c",
             "import subprocess,sys; r=subprocess.run(sys.argv[1:]); print('__CONSOLE_EXIT__='+str(r.returncode),flush=True); sys.stdin.readline()",
             str(ROOT / "target/debug/payment-routing"), "--demo", "--seed", "42", "--scenario", scenario],
            stdin=self.slave, stdout=self.slave, stderr=self.slave,
            env={**os.environ, "TERM": "xterm-256color"}, preexec_fn=setup,
        )
        self.until(lambda s: "minute -- next 0" in s)

    def read(self, seconds=0.1):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            ready, _, _ = select.select([self.master], [], [], max(0, end - time.monotonic()))
            if ready:
                try:
                    data = os.read(self.master, 65536)
                except OSError:
                    break
                if not data:
                    break
                self.raw.extend(data)
                self.screen.feed(data)

    def until(self, predicate, seconds=5):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            self.read(0.03)
            if predicate(self.screen.text()):
                return
        raise AssertionError(self.screen.text())

    def send(self, data):
        os.write(self.master, data)
        self.read()

    def snapshot(self, name):
        OUT.mkdir(parents=True, exist_ok=True)
        (OUT / f"{name}.txt").write_text(self.screen.text() + "\n")

    def resize(self, width, height):
        self.screen = Screen(width, height)
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", height, width, 0, 0))
        os.kill(self.process.pid, signal.SIGWINCH)
        self.read(0.2)

    def finish(self, key):
        self.send(key)
        self.until(lambda _: b"__CONSOLE_EXIT__=" in self.raw)
        assert b"__CONSOLE_EXIT__=0" in self.raw, self.screen.text()
        assert termios.tcgetattr(self.slave) == self.before, "terminal modes not restored"
        assert b"\x1b[?1049l" in self.raw, "alternate screen not restored"
        self.send(b"\n")
        self.process.wait(timeout=5)
        assert self.process.returncode == 0
        os.close(self.master)
        os.close(self.slave)

    def abort(self):
        if self.process.poll() is None:
            os.killpg(self.process.pid, signal.SIGKILL)
            self.process.wait()
        os.close(self.master)
        os.close(self.slave)


def main():
    session = Session()
    try:
        session.send(b".")
        session.until(lambda s: "minute 0 next 1" in s)
        session.send(b"2/SIM-1\r\r")
        session.until(lambda s: "payment investigation" in s and "reserved @0" in s)
        session.snapshot("same-tick-reservation")
        session.send(b"\x1b")
        session.send(b"/\x7f\x7f\x7f\x7f\x7f\r1")
        session.send(b"+++")
        session.until(lambda s: "100 tick/s" in s)
        session.send(b" ")
        session.until(lambda s: "RUNNING" in s and int(re.search(r"next (\d+)", s)[1]) >= 20)
        session.send(b" ")
        session.until(lambda s: "PAUSED" in s)
        minute = re.search(r"next (\d+)", session.screen.text())[1]
        session.read(0.2)
        assert re.search(r"next (\d+)", session.screen.text())[1] == minute
        session.snapshot("balanced-overview")
        session.send(b"2\x1b[H\r")
        session.until(lambda s: "payment investigation" in s)
        session.send(b"\x1b[F")
        session.until(lambda s: "LIFECYCLE" in s)
        session.snapshot("payment-lifecycle")
        session.send(b"\x1b")
        session.send(b"/SIM-1\r")
        session.until(lambda s: "/SIM-1" in s)
        session.snapshot("payment-search")
        session.send(b"3\r")
        session.until(lambda s: "Rail investigation" in s)
        session.snapshot("rail-detail")
        session.send(b"\x1b")
        session.send(b"6")
        session.until(lambda s: "Actual fees USD" in s)
        session.snapshot("comparison")
        session.send(b"r")
        session.until(lambda s: "minute -- next 0" in s)
        session.send(b"n")
        session.until(lambda s: "seed 43" in s)
        session.resize(120, 32)
        session.until(lambda s: "PAYMENT OPS" in s)
        session.snapshot("resized")
        session.resize(40, 10)
        session.until(lambda s: "Resize to at least 80 x 18" in s)
        session.finish(b"q")
    except BaseException:
        session.abort()
        raise
    print("PASS balanced: step, speed, run/pause, payment search/detail, rail detail, comparison, reset/seed, resize, q cleanup")
    for preset, exit_key in [("pressure", b"\x1b"), ("outage", b"\x03"), ("limited", b"q")]:
        session = Session(preset)
        try:
            session.send(b"." * 80)
            session.until(lambda s: "minute 79 next 80" in s)
            session.send(b"5")
            session.snapshot(f"{preset}-optimizer")
            if preset == "limited":
                assert "TRUNCATED" in session.screen.text()
            session.send(b"6")
            session.snapshot(f"{preset}-comparison")
            session.finish(exit_key)
        except BaseException:
            session.abort()
            raise
        print(f"PASS {preset}: 80 deterministic ticks, diagnostics, comparison, exit {exit_key!r}, terminal modes restored")
    print(f"Snapshots: {OUT}")


if __name__ == "__main__":
    main()
