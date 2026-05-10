"""Configuration — ~/.relay/settings.json + environment variables + fallback."""

import os
from dataclasses import dataclass
from pathlib import Path

from exceptions import ConfigError


@dataclass
class Config:
    api_key: str = ""
    base_url: str = "https://api.deepseek.com"
    model: str = "deepseek-chat"
    enter_sends: bool = True
    max_tokens: int = 16384
    max_context_tokens: int = 128000
    context_safety_margin: int = 4000
    retry_max_attempts: int = 3
    retry_base_delay: float = 2.0
    retry_max_delay: float = 60.0
    request_timeout: float = 180.0
    default_shell_timeout: float = 15.0
    max_tool_output: int = 50000
    max_stderr_output: int = 10000
    max_tool_turns: int = 20
    log_level: str = "WARNING"


def load_config() -> Config:
    from relay_config import load_settings, SETTINGS_PATH

    cfg = Config()

    # 1. ~/.relay/settings.json (primary)
    relay_cfg = load_settings()
    if relay_cfg is not None:
        if relay_cfg.get("api_key"):
            cfg.api_key = relay_cfg["api_key"]
        if relay_cfg.get("base_url"):
            cfg.base_url = relay_cfg["base_url"].rstrip("/")
        if relay_cfg.get("model"):
            cfg.model = relay_cfg["model"]
        if "enter_sends" in relay_cfg:
            cfg.enter_sends = bool(relay_cfg["enter_sends"])

    # 2. Environment variables (override everything)
    if os.environ.get("DEEPSEEK_API_KEY"):
        cfg.api_key = os.environ["DEEPSEEK_API_KEY"]
    if os.environ.get("DEEPSEEK_BASE_URL"):
        cfg.base_url = os.environ["DEEPSEEK_BASE_URL"].rstrip("/")
    if os.environ.get("DEEPSEEK_MODEL"):
        cfg.model = os.environ["DEEPSEEK_MODEL"]

    # 3. Fallback: ~/.deepseek/config.toml (backward compat, fills gaps)
    if not relay_cfg:
        deepseek_toml = Path.home() / ".deepseek" / "config.toml"
        if deepseek_toml.exists():
            for line in deepseek_toml.read_text("utf-8").splitlines():
                line = line.strip()
                if not line or line.startswith("#") or "=" not in line:
                    continue
                k, _, v = line.partition("=")
                k, v = k.strip(), v.strip().strip("\"'")
                if k == "api_key" and not cfg.api_key:
                    cfg.api_key = v
                elif k == "base_url" and cfg.base_url == "https://api.deepseek.com":
                    cfg.base_url = v.rstrip("/")
                elif k == "default_text_model" and cfg.model == "deepseek-chat":
                    cfg.model = v

    if not cfg.api_key:
        raise ConfigError(
            "No API key. Run relay to set up ~/.relay/settings.json, "
            "or set DEEPSEEK_API_KEY, "
            "or add api_key to ~/.deepseek/config.toml"
        )

    return cfg
