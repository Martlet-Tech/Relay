"""Relay own config folder (~/.relay/settings.json) management."""

import json
import sys
from pathlib import Path

from exceptions import ConfigError

RELAY_DIR = Path.home() / ".relay"
SETTINGS_PATH = RELAY_DIR / "settings.json"


def settings_path() -> Path:
    return SETTINGS_PATH


def load_settings() -> dict | None:
    if not SETTINGS_PATH.exists():
        return None
    try:
        raw = SETTINGS_PATH.read_text("utf-8")
        return json.loads(raw)
    except (json.JSONDecodeError, OSError) as e:
        raise ConfigError(f"Failed to read {SETTINGS_PATH}: {e}")


def save_settings(data: dict):
    RELAY_DIR.mkdir(parents=True, exist_ok=True)
    tmp = SETTINGS_PATH.with_suffix(".tmp")
    tmp.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", "utf-8")
    tmp.replace(SETTINGS_PATH)


def detect_existing_configs() -> list[dict]:
    """Scan for existing configs we can import from. Returns list of source descriptors."""
    sources = []

    deepseek_toml = Path.home() / ".deepseek" / "config.toml"
    if deepseek_toml.exists():
        sources.append({
            "id": "deepseek",
            "label": f".deepseek/config.toml",
            "path": str(deepseek_toml),
        })

    claude_settings = Path.home() / ".claude" / "settings.json"
    if claude_settings.exists():
        try:
            data = json.loads(claude_settings.read_text("utf-8"))
            if isinstance(data, dict) and len(data) > 0:
                sources.append({
                    "id": "claude",
                    "label": f".claude/settings.json",
                    "path": str(claude_settings),
                })
        except (json.JSONDecodeError, OSError):
            pass

    return sources


def import_from_deepseek(path: str) -> dict:
    """Extract api_key, base_url, model from a DeepSeek config.toml."""
    result = {}
    try:
        for line in Path(path).read_text("utf-8").splitlines():
            line = line.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            k, _, v = line.partition("=")
            k, v = k.strip(), v.strip().strip("\"'")
            if k == "api_key":
                result["api_key"] = v
            elif k == "base_url":
                result["base_url"] = v.rstrip("/")
            elif k == "default_text_model":
                result.setdefault("model", v)
    except OSError as e:
        raise ConfigError(f"Cannot read {path}: {e}")
    return result


def import_from_claude(path: str) -> dict:
    """Try to extract API info from .claude/settings.json. May return partial data."""
    result = {}
    try:
        data = json.loads(Path(path).read_text("utf-8"))
    except (json.JSONDecodeError, OSError):
        return result

    if not isinstance(data, dict):
        return result

    claude_projects = data if "projects" not in data else data.get("projects", {})

    for project_key, project_val in (claude_projects.items() if isinstance(claude_projects, dict) else {}):
        if not isinstance(project_val, dict):
            continue
        for field in ("api_key", "model", "base_url"):
            if field in project_val and isinstance(project_val[field], str):
                result[field] = project_val[field]
    return result


def _ask(prompt: str, default: str = "") -> str:
    """Read a line from stdin with an optional prompt."""
    try:
        return input(prompt).strip() or default
    except (EOFError, KeyboardInterrupt):
        print()
        sys.exit(1)


def first_time_setup() -> dict:
    """Interactive first-launch setup — detect existing configs, ask user, write settings."""
    print()
    print("  First time running relay — let's set up ~/.relay/settings.json")
    print("  " + "-" * 48)
    print()

    sources = detect_existing_configs()
    imported = {}

    if sources:
        print("  Found existing configs:")
        for i, s in enumerate(sources, 1):
            print(f"    [{i}] {s['label']}")
        print(f"    [{len(sources) + 1}] Skip — enter manually")
        print()

        choice = _ask(f"  Import from (1-{len(sources) + 1}) or press Enter for 1: ", "1")
        try:
            idx = int(choice) - 1
        except ValueError:
            idx = 0

        if 0 <= idx < len(sources):
            selected = sources[idx]
            print(f"  Importing from {selected['label']}...")
            if selected["id"] == "deepseek":
                imported = import_from_deepseek(selected["path"])
            elif selected["id"] == "claude":
                imported = import_from_claude(selected["path"])
            if imported:
                print(f"    Got: {', '.join(imported.keys())}")
            else:
                print("    Nothing usable found in that config.")
        else:
            print("  OK, manual entry it is.")

    settings = {}
    settings["api_key"] = imported.get("api_key", "") or _ask("  API key (sk-...): ")
    settings["base_url"] = imported.get("base_url", "") or _ask(
        "  Base URL (default: https://api.deepseek.com): ",
        "https://api.deepseek.com",
    )
    settings["model"] = imported.get("model", "") or _ask(
        "  Model (default: deepseek-chat): ",
        "deepseek-chat",
    )

    print()
    enter_raw = _ask("  Enter sends, Shift+Enter = newline? [Y/n]: ", "y")
    settings["enter_sends"] = enter_raw.lower() not in ("n", "no")

    save_settings(settings)
    print(f"\n  Saved to {SETTINGS_PATH}")
    print()

    return settings


def ensure_settings() -> dict:
    """Return settings dict — load existing or run first-time setup."""
    existing = load_settings()
    if existing is not None:
        return existing
    return first_time_setup()
