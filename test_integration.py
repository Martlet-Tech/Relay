"""Integration tests for relay agent - runs against the real API."""

import sys, os, json

# Make sure we can import from Relay/
sys.path.insert(0, os.path.dirname(__file__))

from config import load_config, ConfigError
from env_detect import detect_environment, build_system_prompt
from session import Session
from client import stream_chat_completion
from tools import TOOL_DEFS, execute_tool

PASS = 0
FAIL = 0


def ok(name, msg=""):
    global PASS
    PASS += 1
    print(f"  OK  {name}" + (f"  ({msg})" if msg else ""))


def ng(name, detail):
    global FAIL
    FAIL += 1
    print(f"  FAIL  {name}: {detail}")


def test_config():
    try:
        cfg = load_config()
        assert cfg.api_key, "api_key is empty"
        assert cfg.base_url, "base_url is empty"
        assert cfg.model, "model is empty"
        ok("config loads", f"{cfg.model} @ {cfg.base_url}")
    except ConfigError as e:
        ng("config", str(e))
        raise  # can't proceed


def test_env_detect():
    env = detect_environment()
    assert env["platform"], "no platform"
    assert env["os"], "no os"
    assert env["default_shell"], "no shell"
    ok("env detection", f"{env['os']} / {env['default_shell']}")
    prompt = build_system_prompt(env)
    assert "## Environment" in prompt
    ok("system prompt built", f"{len(prompt)} chars")


def test_tools():
    # shell
    r = execute_tool("shell", '{"command":"echo hello","timeout_ms":5000}')
    assert "hello" in r, f"shell failed: {r}"
    ok("tool: shell", r.strip())

    # read non-existent
    r = execute_tool("read", '{"path":"/tmp/nonexistent_relay_test_file_xyz"}')
    assert "not found" in r
    ok("tool: read (missing file handled)")

    # write + read
    r = execute_tool("write", '{"path":"_test_relay.txt","content":"hello relay"}')
    assert "Wrote" in r, f"write failed: {r}"
    r = execute_tool("read", '{"path":"_test_relay.txt"}')
    assert "hello relay" in r, f"read back failed: {r}"
    os.remove("_test_relay.txt")
    ok("tool: write + read round-trip")

    # glob
    r = execute_tool("glob", '{"pattern":"*.py","path":"."}')
    assert ".py" in r, f"glob returned nothing: {r}"
    ok("tool: glob")

    r = execute_tool("grep", '{"pattern":"def test_","path":"."}')
    assert "test_integration" in r
    ok("tool: grep (found self)")

    # unknown tool
    r = execute_tool("nonexistent", "{}")
    assert "unknown" in r
    ok("tool: unknown tool handled")


def test_session():
    from config import Config
    cfg = Config()
    s = Session(cfg, system_prompt="test system prompt")
    assert len(s.messages) == 1
    assert s.messages[0]["role"] == "system"
    s.add_user_message("hello")
    assert len(s.messages) == 2
    s.add_assistant_message("world")
    assert len(s.messages) == 3
    popped = s.pop_last_user_message()
    assert popped["role"] == "user"
    ok("session: lifecycle")
    ok("session: token estimate", f"~{s.total_tokens()} tokens")


def test_api_basic():
    """Send one message, expect a text response back."""
    cfg = load_config()
    system_prompt = build_system_prompt(detect_environment())
    session = Session(cfg, system_prompt=system_prompt)
    session.add_user_message("Say exactly two words: hello relay. Nothing else.")

    content = []
    tool_calls = []
    error = None
    for kind, data in stream_chat_completion(session.messages, TOOL_DEFS, cfg):
        if kind == "content":
            content.append(data)
        elif kind == "tool_call":
            tool_calls.append(data)
        elif kind == "error":
            error = data
            break

    if error:
        ng("api basic", error)
        return

    full = "".join(content)
    assert len(full) > 0, "empty response"
    assert "hello" in full.lower() or "relay" in full.lower(), (
        f"unexpected: {full[:100]}"
    )
    ok("api: basic text response", f"{len(full)} chars, {len(tool_calls)} tool calls")


def test_api_tool_call():
    """The model should call shell() when asked to list files."""
    cfg = load_config()
    system_prompt = build_system_prompt(detect_environment())
    session = Session(cfg, system_prompt=system_prompt)
    session.add_user_message("Run the shell tool once: list .py files in the current directory with dir or ls, whichever works on this OS. Report what you find.")

    content = []
    tool_calls = []
    error = None
    for kind, data in stream_chat_completion(session.messages, TOOL_DEFS, cfg):
        if kind == "content":
            content.append(data)
        elif kind == "tool_call":
            tool_calls.append(data)
        elif kind == "error":
            error = data
            break

    if error:
        ng("api tool_call", error)
        return

    assert len(tool_calls) >= 1, f"model did not call a tool: content={''.join(content)[:100]}"
    names = [tc["name"] for tc in tool_calls]
    ok("api: model called tool(s)", ", ".join(names))


print()
print("  relay integration tests")
print("  " + "-" * 40)
print()

tests = [test_config, test_env_detect, test_tools, test_session, test_api_basic, test_api_tool_call]
for t in tests:
    try:
        t()
    except Exception as e:
        ng(t.__name__, str(e))

print()
print(f"  {PASS} passed, {FAIL} failed")
print()
sys.exit(0 if FAIL == 0 else 1)
