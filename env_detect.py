"""Detect the runtime environment and build a system prompt."""

import os
import platform
import shutil
import sys


def detect_environment() -> dict:
    """Gather key facts about the current environment."""
    info = {}

    # OS
    info["platform"] = sys.platform
    info["os"] = platform.system()
    info["os_version"] = platform.version() or platform.release()

    # Default shell (what subprocess shell=True will use)
    comspec = os.environ.get("COMSPEC", "")
    if comspec:
        info["default_shell"] = os.path.basename(comspec).lower()
    elif shutil.which("bash"):
        info["default_shell"] = "bash"
    elif shutil.which("sh"):
        info["default_shell"] = "sh"
    else:
        info["default_shell"] = "unknown"

    # Available shells
    candidates = ["cmd.exe", "powershell.exe", "bash", "zsh", "fish", "sh"]
    info["available_shells"] = [s for s in candidates if shutil.which(s)]

    # Python
    info["python_version"] = sys.version.split()[0]
    info["python_path"] = shutil.which("python") or shutil.which("python3") or ""

    # Working directory
    info["cwd"] = os.getcwd()

    # Common tooling
    for tool in ["git", "node", "npm", "cargo", "go", "make", "gcc", "clang"]:
        info[tool] = shutil.which(tool) is not None

    return info


def build_system_prompt(env: dict) -> str:
    """Build a system prompt describing the detected environment."""
    lines = [
        "You are an AI agent running on the local machine. "
        "You have access to tools including shell execution, file read/write, "
        "and search. Use them to accomplish the user's requests."
    ]

    # OS
    os_str = env.get("os", "?")
    ver = env.get("os_version")
    if ver:
        os_str += f" ({ver})"
    lines.append(f"\n## Environment")
    lines.append(f"- OS: {os_str}")
    lines.append(f"- Default shell: {env.get('default_shell', '?')}")
    avail = env.get("available_shells")
    if avail:
        lines.append(f"- Available shells: {', '.join(avail)}")
    lines.append(f"- Python {env.get('python_version', '?')}")
    lines.append(f"- Working directory: {env.get('cwd', '?')}")

    avail_tools = [k for k in ("git", "node", "npm", "cargo", "go", "make") if env.get(k)]
    if avail_tools:
        lines.append(f"- Tools available: {', '.join(avail_tools)}")

    # Shell-specific guidance
    shell = env.get("default_shell", "")
    if shell in ("cmd.exe",):
        lines.extend([
            "",
            "## Shell notes (cmd.exe)",
            "- `dir`  → list files",
            "- `cd`   → show or change directory",
            "- `type <file>`  → read a file's contents",
            "- `python <script>`  → run a Python script",
            "- Paths use backslashes; forward slashes also work",
        ])
    elif shell in ("bash", "zsh", "fish", "sh"):
        lines.extend([
            "",
            f"## Shell notes ({shell})",
            "- `ls`       → list files",
            "- `pwd`      → current directory",
            "- `cat`      → read a file",
            "- `python3`  → run a Python script",
            "- Standard Unix paths with forward slashes",
        ])
    elif shell in ("powershell.exe",):
        lines.extend([
            "",
            "## Shell notes (PowerShell)",
            "- `ls` / `dir`  → list files",
            "- `pwd`         → current directory",
            "- `cat` / `Get-Content`  → read a file",
            "- `python`      → run a Python script",
        ])

    lines.extend([
        "",
        "## Rules",
        "- You may call multiple tools per turn if needed.",
        "- When a command fails, diagnose and try an alternative.",
        "- Prefer the default shell unless there's a specific reason to switch.",
    ])

    return "\n".join(lines)
