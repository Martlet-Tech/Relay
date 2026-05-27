#!/usr/bin/env python3
"""
relay — AI agent with local tool calling.

Dual-mode terminal UI:
  - Application mode (prompt_toolkit): full-screen layout with bordered input + toolbar
  - Terminal mode (input()/print()): classic REPL fallback
"""

import json
import logging
import os
import shutil
import sys
import time
from functools import partial

from config import load_config, ConfigError
from env_detect import detect_environment, build_system_prompt
from relay_config import ensure_settings
from session import Session
from client import stream_chat_completion
from tools import TOOL_DEFS, execute_tool
from spinner import RelaySpinner

logger = logging.getLogger(__name__)

_SEP = "  " + "─" * 48

# ── Avatar boxes ──

_BLUE = "\033[34m"
_GREEN = "\033[32m"
_RESET = "\033[0m"


def _box_lines(name, color_code):
    inner = f" {name} "
    border = f"{color_code}+{'-' * len(inner)}+{_RESET}"
    middle = f"{color_code}|{inner}|{_RESET}"
    return [border, middle, border]


def _agent_box():
    return [f"  {l}" for l in _box_lines("Relay", _BLUE)]


def _box_visual_width(name):
    """Width of the avatar box content (without ANSI codes)."""
    return len(name) + 4  # e.g. "+----+" for name="zt"


def _user_box(username):
    raw = _box_lines(username, _GREEN)
    w = shutil.get_terminal_size().columns
    vis_w = _box_visual_width(username)
    pad = " " * max(0, w - 2 - vis_w)
    return [f"  {pad}{l}" for l in raw]


def _stats_line(elapsed, usage=None, session=None):
    parts = [f"{elapsed:.1f}s"]
    if usage:
        total = usage.get("total_tokens") or usage.get("completion_tokens")
        if total:
            parts.append(f"{total}tok")
    if session:
        used = session.total_tokens()
        max_tok = session._max_tokens
        if max_tok:
            pct = int(100 - (used / max_tok * 100))
            parts.append(f"ctx:{pct}%")
    return "  ".join(parts)


# ── Terminal mode helpers ──

HAS_PT = False
try:
    from prompt_toolkit.key_binding import KeyBindings
    from prompt_toolkit.shortcuts import PromptSession

    HAS_PT = True
except ImportError:
    pass


def _make_prompt_session(enter_sends: bool, cfg):
    from prompt_toolkit.key_binding import KeyBindings
    from prompt_toolkit.shortcuts import PromptSession
    from prompt_toolkit.styles import Style

    kb = KeyBindings()

    if enter_sends:
        @kb.add("enter")
        def _submit(event):
            event.current_buffer.validate_and_handle()

        @kb.add("escape", "enter")
        def _newline(event):
            event.current_buffer.insert_text("\n")
    else:
        @kb.add("enter")
        def _newline(event):
            event.current_buffer.insert_text("\n")

        @kb.add("escape", "enter")
        def _submit(event):
            event.current_buffer.validate_and_handle()

    def _toolbar():
        return [("class:toolbar", f" Relay | {cfg.model} ")]

    style = Style.from_dict({"toolbar": "bg:#005080 fg:#ffffff"})

    try:
        return PromptSession(
            multiline=True,
            key_bindings=kb,
            bottom_toolbar=_toolbar,
            style=style,
        )
    except Exception:
        return None


def _clear_lines(n):
    for _ in range(n):
        sys.stdout.write("\033[1A\033[2K")
    sys.stdout.flush()


# ── Core processing (shared between terminal and UI mode) ──


def process_turn(cfg, session, username="user", ui=None):
    """Process one user turn.

    Args:
        ui: RelayUI for Application mode, or None for terminal mode.
    """
    out = ui.append_line if ui else None

    for turn in range(cfg.max_tool_turns):
        session.ensure_context_fit()

        content_chunks = []
        reasoning_chunks = []
        tool_calls = []
        warnings = []
        usage_data = None
        turn_start = time.time()

        # ── Spinner / thinking indicator ──
        if ui:
            ui.set_processing(True)
        else:
            sys.stdout.write("\n")
            sys.stdout.flush()
            spinner = RelaySpinner()
            spinner.start()

        got_first_event = False

        for kind, data in stream_chat_completion(session.messages, TOOL_DEFS, cfg):
            if not got_first_event:
                got_first_event = True
                if ui:
                    for l in _agent_box():
                        ui.append_ansi(l)
                else:
                    spinner.stop()
                    sys.stdout.write("\r\033[K")
                    for l in _agent_box():
                        sys.stdout.write(l + "\n")
                    sys.stdout.write("  ")
                    sys.stdout.flush()

            if kind == "content":
                content_chunks.append(data)
                if ui:
                    ui.append_text(data)
                else:
                    sys.stdout.write(data)
                    sys.stdout.flush()

            elif kind == "reasoning":
                reasoning_chunks.append(data)

            elif kind == "tool_call":
                tool_calls.append(data)

            elif kind == "usage":
                usage_data = data

            elif kind == "warning":
                warnings.append(data)

            elif kind == "error":
                if ui:
                    ui.append_line(f"\n  Error: {data}")
                else:
                    sys.stdout.write("\n  Error: %s\n" % data)
                session.pop_last_user_message()
                return

        if not got_first_event:
            if not ui:
                spinner.stop()
                sys.stdout.write("\r\033[K")
                sys.stdout.flush()

        elapsed = time.time() - turn_start
        stats = _stats_line(elapsed, usage_data, session)

        if ui:
            ui.append_line()
        else:
            print()

        for w in warnings:
            if ui:
                ui.append_line(f"  ! {w}")
            else:
                print(f"  ! {w}")

        if not tool_calls:
            if content_chunks:
                if ui:
                    ui.append_line(f"  ──  {stats}")
                else:
                    print(f"  ──  {stats}")
                session.add_assistant_message(
                    "".join(content_chunks),
                    reasoning="".join(reasoning_chunks) or None,
                )
            else:
                if ui:
                    ui.append_line("  ──")
                else:
                    print("  ──")
            return

        asst_msg = {
            "role": "assistant",
            "content": "".join(content_chunks) or None,
        }
        reasoning_text = "".join(reasoning_chunks)
        if reasoning_text:
            asst_msg["reasoning_content"] = reasoning_text
        asst_msg["tool_calls"] = [
            {
                "id": tc["id"],
                "type": "function",
                "function": {"name": tc["name"], "arguments": tc["args"]},
            }
            for tc in tool_calls
        ]
        session.messages.append(asst_msg)

        for tc in tool_calls:
            try:
                snippet = json.dumps(json.loads(tc["args"]), ensure_ascii=False)
            except Exception:
                snippet = tc.get("args", "")
            if ui:
                ui.append_line(f"  → {tc['name']}({snippet})")
            else:
                print(f"  → {tc['name']}({snippet})")
            result = execute_tool(tc["name"], tc["args"])
            display = result[:300] + ("..." if len(result) > 300 else "")
            if ui:
                ui.append_line(f"  {display}")
            else:
                print(f"  {display}")
            session.add_tool_result(tc["id"], result)
        if ui:
            ui.append_line()
        else:
            print()

    msg = f"  ! Reached max {cfg.max_tool_turns} tool turns\n"
    if ui:
        ui.append_line(msg)
    else:
        print(msg)
    session.pop_last_user_message()


# ── Commands ──


def _cmd(line, session, cfg, ui=None):
    """Handle a /command. Returns True to continue, False to exit."""
    out = ui.append_line if ui else print
    parts = line[1:].split()
    cmd = parts[0].lower()
    if cmd in ("exit", "quit"):
        return False
    if cmd == "clear":
        session.clear()
        out("  --- cleared\n")
        return True
    if cmd == "model" and len(parts) > 1:
        cfg.model = parts[1]
        out(f"  --- switched to {cfg.model}\n")
        return True
    if cmd == "tools":
        for t in TOOL_DEFS:
            fn = t["function"]
            out(f"  {fn['name']}: {fn['description']}")
        out()
        return True
    if cmd == "tokens":
        out(f"  ~{session.total_tokens()} / {session._max_tokens} tokens ({cfg.model})\n")
        return True
    out(f"  Unknown command: /{cmd}\n")
    return True


# ── Terminal mode main loop ──


def _terminal_loop(cfg, session, username):
    """Classic REPL mode (input() + print())."""
    _print_banner(cfg, username)
    ps = _make_prompt_session(cfg.enter_sends, cfg) if HAS_PT else None

    while True:
        box_w = shutil.get_terminal_size().columns - 4
        print(f"  ┌{'─' * (box_w - 2)}┐")

        try:
            if ps:
                line = ps.prompt("  │ > ").strip()
            else:
                line = input("  │ > ").strip()
        except KeyboardInterrupt:
            _clear_lines(3 if ps else 2)
            continue
        except EOFError:
            _clear_lines(3 if ps else 2)
            print()
            break

        _clear_lines(3 if ps else 2)

        if not line:
            continue

        if line.startswith("/"):
            if not _cmd(line, session, cfg):
                break
            continue

        # Show user message
        raw_box = _box_lines(username, _GREEN)
        w = shutil.get_terminal_size().columns
        pad = " " * max(0, w - 2 - len(raw_box[0]))
        for l in raw_box:
            print(f"  {pad}{l}")
        print(f"  {pad}{line}")

        session.add_user_message(line)

        try:
            process_turn(cfg, session, username)
        except KeyboardInterrupt:
            print("\n  --- interrupted ---")
            session.pop_last_user_message()


def _print_banner(cfg, username):
    """Print startup banner (terminal mode only)."""
    env = detect_environment()
    avail = ", ".join(k for k in ("git", "node", "npm", "cargo", "go", "make") if env.get(k))
    os_ver = env.get("os", "")
    if env.get("os_version"):
        os_ver += f" ({env['os_version']})"

    lines = [
        f"User:  {username}",
        f"Model: {cfg.model}",
        f"OS:    {os_ver}",
        f"Shell: {env.get('default_shell', '?')}",
        f"CWD:   {env.get('cwd', '?')}",
    ]
    if avail:
        lines.append(f"Tools: {avail}")

    cmds = "/exit  /clear  /model <name>  /tools  /tokens"
    w = max(max(len(l) for l in lines + [cmds]) + 4, 54)
    p = w - 2
    hr = "+" + "-" * (w - 2) + "+"

    print()
    print(f"  {hr}")
    print(f"  |{'relay':^{p}}|")
    print(f"  {hr}")
    for l in lines:
        print(f"  | {l:<{p-1}}|")
    print(f"  {hr}")
    print(f"  | {cmds:<{p-1}}|")
    print(f"  {hr}")
    print()


# ── Application mode (prompt_toolkit full-screen) ──


def _app_main(cfg, session, username):
    """Full-screen Application mode using prompt_toolkit."""
    from ui import RelayUI

    env = detect_environment()

    def on_submit(text):
        if text.startswith("/"):
            if not _cmd(text, session, cfg, ui=app):
                app.exit()
            return

        session.add_user_message(text)

        # Show user message
        raw_box = _box_lines(username, _GREEN)
        w = shutil.get_terminal_size().columns
        pad = " " * max(0, w - 2 - len(raw_box[0]))
        for l in raw_box:
            app.append_ansi(f"  {pad}{l}")
        app.append_line(f"  {pad}{text}")

        # Run processing in background thread
        import asyncio

        app.set_processing(True)
        asyncio.get_event_loop().run_in_executor(None, _app_process)

    def _app_process():
        try:
            process_turn(cfg, session, username, ui=app)
        finally:
            app.set_processing(False)

    app = RelayUI(cfg, session, username, env, on_submit=on_submit)
    app.run()


# ── Entry point ──


def main():
    ensure_settings()
    try:
        cfg = load_config()
    except ConfigError as e:
        print(f"Configuration error: {e}", file=sys.stderr)
        sys.exit(1)

    logging.getLogger().setLevel(
        getattr(logging, cfg.log_level.upper(), logging.WARNING)
    )
    if len(sys.argv) == 3 and sys.argv[1] == "--model":
        cfg.model = sys.argv[2]

    system_prompt = build_system_prompt(detect_environment())
    session = Session(cfg, system_prompt=system_prompt)
    username = os.environ.get("USERNAME") or os.environ.get("USER") or "user"

    if HAS_PT:
        # Application mode (full-screen TUI)
        try:
            _app_main(cfg, session, username)
        except ImportError:
            _terminal_loop(cfg, session, username)
    else:
        _terminal_loop(cfg, session, username)


if __name__ == "__main__":
    main()
