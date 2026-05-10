"""Tool definitions (OpenAI-compatible schema) and execution."""

import json
import logging
import re
import subprocess
from pathlib import Path

logger = logging.getLogger(__name__)

# ── Tool definitions ──

TOOL_DEFS = [
    {
        "type": "function",
        "function": {
            "name": "shell",
            "description": "Run a shell command and get its output (stdout + stderr).",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "Shell command to run"},
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Max execution time in ms (default 15000)",
                        "default": 15000,
                    },
                },
                "required": ["command"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "read",
            "description": "Read a text file from disk. Refuses binary files (null bytes).",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path"},
                },
                "required": ["path"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "write",
            "description": "Write text content to a file. Creates parent dirs. Overwrites existing.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path"},
                    "content": {"type": "string", "description": "Content to write"},
                },
                "required": ["path", "content"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "glob",
            "description": "List files matching a glob pattern under a directory.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Glob pattern, e.g. **/*.py"},
                    "path": {
                        "type": "string",
                        "description": "Directory to search (default: current directory)",
                    },
                },
                "required": ["pattern"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "grep",
            "description": "Search file contents with a regex pattern. Skips binary files.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Regex pattern"},
                    "path": {"type": "string", "description": "File or directory to search"},
                    "glob": {
                        "type": "string",
                        "description": "Optional: only search files matching this glob, e.g. *.py",
                    },
                },
                "required": ["pattern", "path"],
            },
        },
    },
]

# ── Execution ──

_MAX_OUTPUT = 50000
_MAX_STDERR = 10000
_DEFAULT_TIMEOUT_S = 15.0
_MAX_TIMEOUT_S = 300.0


def _is_binary(path):
    try:
        with open(path, "rb") as f:
            return b"\0" in f.read(8192)
    except OSError:
        return False


def _tool_shell(args):
    cmd = args.get("command", "").strip()
    if not cmd:
        return "Error: empty command"
    timeout_s = min(
        args.get("timeout_ms", int(_DEFAULT_TIMEOUT_S * 1000)) / 1000,
        _MAX_TIMEOUT_S,
    )
    r = subprocess.run(cmd, shell=True, capture_output=True, text=True, timeout=timeout_s)
    parts = []
    if r.stdout:
        parts.append(r.stdout[:_MAX_OUTPUT])
    if r.stderr:
        parts.append(f"--- stderr ---\n{r.stderr[:_MAX_STDERR]}")
    parts.append(f"--- exit code: {r.returncode}")
    return "\n".join(parts).strip()


def _tool_read(args):
    path = args.get("path", "").strip()
    if not path:
        return "Error: empty path"
    p = Path(path).expanduser().resolve()
    if not p.exists():
        return f"Error: file not found: {p}"
    if not p.is_file():
        return f"Error: not a file: {p}"
    if _is_binary(p):
        return f"Error: binary file (refusing to read): {p}"
    try:
        return p.read_text("utf-8", errors="replace")[:100000]
    except Exception as e:
        return f"Error: cannot read {p}: {e}"


def _tool_write(args):
    path = args.get("path", "").strip()
    content = args.get("content", "")
    if not path:
        return "Error: empty path"
    p = Path(path).expanduser().resolve()
    try:
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(content, "utf-8")
        return f"Wrote {len(content)} bytes to {p}"
    except OSError as e:
        return f"Error: cannot write to {p}: {e}"


def _tool_glob(args):
    root = Path(args.get("path", ".")).expanduser().resolve()
    pattern = args.get("pattern", "")
    if not pattern:
        return "Error: empty pattern"
    try:
        matches = sorted(root.rglob(pattern))
    except (OSError, ValueError) as e:
        return f"Error: glob failed: {e}"
    if not matches:
        return f"No files matching '{pattern}' in {root}"
    lines = []
    for m in matches[:200]:
        try:
            lines.append(str(m.relative_to(root)))
        except ValueError:
            lines.append(str(m))
    return "\n".join(lines)


def _tool_grep(args):
    path = args.get("path", "").strip()
    pattern = args.get("pattern", "")
    glob_filter = args.get("glob", "")
    if not pattern:
        return "Error: empty pattern"
    try:
        regex = re.compile(pattern)
    except re.error as e:
        return f"Error: invalid regex: {e}"
    root = Path(path).expanduser().resolve()
    if not root.exists():
        return f"Error: path not found: {root}"
    if root.is_file():
        files = [root]
    else:
        try:
            files = sorted(root.rglob(glob_filter)) if glob_filter else sorted(root.rglob("*"))
        except OSError as e:
            return f"Error: cannot list directory {root}: {e}"
    results = []
    for f in files:
        if not f.is_file() or _is_binary(f):
            continue
        try:
            text = f.read_text("utf-8", errors="replace")
            for i, line in enumerate(text.splitlines(), 1):
                if regex.search(line):
                    results.append(f"{f}:{i}: {line[:200]}")
                    if len(results) >= 200:
                        return "\n".join(results) + "\n--- truncated at 200 matches ---"
        except Exception:
            continue
    return "\n".join(results) if results else f"No matches for '{pattern}' in {path}"


_HANDLERS = {
    "shell": _tool_shell,
    "read": _tool_read,
    "write": _tool_write,
    "glob": _tool_glob,
    "grep": _tool_grep,
}


def execute_tool(name, args_str):
    """Execute a tool call and return the result text."""
    try:
        args = json.loads(args_str) if isinstance(args_str, str) else args_str
    except json.JSONDecodeError as e:
        return f"Error: invalid arguments JSON — {e}"
    handler = _HANDLERS.get(name)
    if not handler:
        return f"Error: unknown tool '{name}'"
    try:
        return handler(args)
    except subprocess.TimeoutExpired:
        return "Error: command timed out"
    except Exception as e:
        logger.debug("Tool '%s' failed: %s", name, e)
        return f"Error: tool '{name}' failed — {e}"
