"""Thinking spinner — relay click-clack animation."""

import threading
import time
import sys


class RelaySpinner:
    """A single-pole double-throw (SPDT) relay spinner.

    Shows a wiper (o) switching between NC and NO, with a bulb
    that lights when COM connects to NO.

    Frame 1 (click):  NC=o=COM  NO     ( )  COM↔NC, bulb off
    Frame 2 (clack):  NC  COM=o=NO     (*)  COM↔NO, bulb on
    """

    _FRAMES = [
        "  NC=o=COM  NO     ( )  click",
        "  NC  COM=o=NO     (*)  clack",
    ]

    def __init__(self, delay: float = 0.35):
        self._delay = delay
        self._running = False
        self._thread: threading.Thread | None = None

    def start(self):
        self._running = True
        self._thread = threading.Thread(target=self._spin, daemon=True)
        self._thread.start()

    def stop(self):
        self._running = False
        if self._thread:
            self._thread.join(0.5)
        sys.stdout.write("\r" + " " * 60 + "\r")
        sys.stdout.flush()

    def _spin(self):
        i = 0
        while self._running:
            sys.stdout.write("\r" + self._FRAMES[i % 2])
            sys.stdout.flush()
            time.sleep(self._delay)
            i += 1
