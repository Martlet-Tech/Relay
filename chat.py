#!/usr/bin/env python3
"""
relay — AI agent with local tool calling.
Zero external deps (prompt_toolkit optional: pip install prompt_toolkit).

Usage:
    python chat.py
    python chat.py --model deepseek-reasoner
"""

import json
import logging
import sys
import time

from config import load_config, ConfigError
from env_detect import detect_environment, build_system_prompt
from relay_config import ensure_settings
from session import Session
from client import stream_chat_completion
from tools import TOOL_DEFS, execute_tool
from spinner import RelaySpinner

logger = logging.getLogger(__name__)

_SEP = "  " + "─" * 48

# Optional: prompt_toolkit gives multi-line input, history, bottom-anchored prompt
HAS_PT = False
try:
    from prompt_toolkit.key_binding import KeyBindings
    from prompt_toolkit.shortcuts import PromptSession

    HAS_PT = True
except ImportError:
    pass


def _make_prompt_session(enter_sends: bool):
    """Build a PromptSession with Enter/Alt+Enter configured per preference."""
    from prompt_toolkit.key_binding import KeyBindings
    from prompt_toolkit.shortcuts import PromptSession

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

    return PromptSession(multiline=True, key_bindings=kb)


def main():
    ensure_settings()
    try:
        cfg = load_config()
    except ConfigError as e:
        print(f"Configuration error: {e}", file=sys.stderr)
        sys.exit(1)

    logging.getLogger().setLevel(getattr(logging, cfg.log_level.upper(), logging.WARNING))
    if len(sys.argv) == 3 and sys.argv[1] == "--model":
        cfg.model = sys.argv[2]

    env = detect_environment()
    system_prompt = build_system_prompt(env)

    session = Session(cfg, system_prompt=system_prompt)
    _print_banner(env, cfg)

    ps = _make_prompt_session(cfg.enter_sends) if HAS_PT else None

    while True:
        try:
            if ps:
                line = ps.prompt(f"\n{_SEP}\n > ").strip()
            else:
                line = input(f"\n{_SEP}\n > ").strip()
        except KeyboardInterrupt:
            print()
            continue
        except EOFError:
            print()
            break

        if not line:
            continue

        if line.startswith("/"):
            if not _cmd(line, session, cfg):
                break
            continue

        session.add_user_message(line)
        try:
            _process_turn(cfg, session)
        except KeyboardInterrupt:
            print("\n  --- interrupted ---")
            session.pop_last_user_message()


def _cmd(line, session, cfg):
    parts = line[1:].split()
    cmd = parts[0].lower()
    if cmd in ("exit", "quit"):
        return False
    if cmd == "clear":
        session.clear()
        print("  --- cleared\n")
        return True
    if cmd == "model" and len(parts) > 1:
        cfg.model = parts[1]
        print(f"  --- switched to {cfg.model}\n")
        return True
    if cmd == "tools":
        for t in TOOL_DEFS:
            fn = t["function"]
            print(f"  {fn['name']}: {fn['description']}")
        print()
        return True
    if cmd == "tokens":
        print(f"  ~{session.total_tokens()} / {session._max_tokens} tokens ({cfg.model})\n")
        return True
    print(f"  Unknown command: /{cmd}\n")
    return True


def _stats_line(elapsed, usage=None, session=None):
    """Build a compact stats string e.g. '3.2s  150tok  ctx:72%'."""
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


def _process_turn(cfg, session):
    """Run one user turn — may involve multiple tool-call rounds."""
    for turn in range(cfg.max_tool_turns):
        session.ensure_context_fit()

        content_chunks = []
        reasoning_chunks = []
        tool_calls = []
        warnings = []
        usage_data = None
        turn_start = time.time()

        # Bottom-anchored footer: spinner sits on line above SEP
        sys.stdout.write("\n")
        sys.stdout.flush()
        spinner = RelaySpinner()
        spinner.start()
        sys.stdout.write("\n" + "  " + "─" * 48 + "\033[A")
        sys.stdout.flush()

        got_first_event = False

        for kind, data in stream_chat_completion(session.messages, TOOL_DEFS, cfg):
            if not got_first_event:
                spinner.stop()
                got_first_event = True
                sys.stdout.write("\r\033[K  ")
                sys.stdout.flush()

            if kind == "content":
                content_chunks.append(data)
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
                sys.stdout.write("\n  Error: %s\n" % data)
                session.pop_last_user_message()
                return

        if not got_first_event:
            spinner.stop()
            sys.stdout.write("\r\033[K")
            sys.stdout.flush()

        elapsed = time.time() - turn_start
        stats = _stats_line(elapsed, usage_data, session)

        print()

        for w in warnings:
            print(f"  ! {w}")

        if not tool_calls:
            if content_chunks:
                print(f"  ──  {stats}\n")
                session.add_assistant_message(
                    "".join(content_chunks),
                    reasoning="".join(reasoning_chunks) or None,
                )
            else:
                print("  ──\n")
            return

        asst_msg = {
            "role": "assistant",
            "content": "".join(content_chunks) or None,
        }
        reasoning_text = "".join(reasoning_chunks)
        if reasoning_text:
            asst_msg["reasoning_content"] = reasoning_text
        asst_msg["tool_calls"] = [
            {"id": tc["id"], "type": "function", "function": {"name": tc["name"], "arguments": tc["args"]}}
            for tc in tool_calls
        ]
        session.messages.append(asst_msg)

        for tc in tool_calls:
            try:
                snippet = json.dumps(json.loads(tc["args"]), ensure_ascii=False)
            except Exception:
                snippet = tc.get("args", "")
            print(f"  → {tc['name']}({snippet})")
            result = execute_tool(tc["name"], tc["args"])
            display = result[:300] + ("..." if len(result) > 300 else "")
            print(f"  {display}")
            session.add_tool_result(tc["id"], result)
        print()

    print(f"  ! Reached max {cfg.max_tool_turns} tool turns\n")
    session.pop_last_user_message()


def _print_banner(env, cfg):
    avail = ", ".join(k for k in ("git", "node", "npm", "cargo", "go", "make") if env.get(k))
    os_ver = env.get("os", "")
    if env.get("os_version"):
        os_ver += f" ({env['os_version']})"

    lines = [
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


if __name__ == "__main__":
    main()
