"""Configuration — ~/.deepseek/config.toml + environment variables."""

import os
from dataclasses import dataclass
from pathlib import Path

from exceptions import ConfigError


@dataclass
class Config:
    api_key: str = ""
    base_url: str = "https://api.deepseek.com"
    model: str = "deepseek-chat"
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
    cfg = Config()

    # 1. Config file (~/.deepseek/config.toml)
    cfg_path = Path.home() / ".deepseek" / "config.toml"
    if cfg_path.exists():
        for line in cfg_path.read_text("utf-8").splitlines():
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

    # 2. Environment variables (override file)
    if os.environ.get("DEEPSEEK_API_KEY"):
        cfg.api_key = os.environ["DEEPSEEK_API_KEY"]
    if os.environ.get("DEEPSEEK_BASE_URL"):
        cfg.base_url = os.environ["DEEPSEEK_BASE_URL"].rstrip("/")
    if os.environ.get("DEEPSEEK_MODEL"):
        cfg.model = os.environ["DEEPSEEK_MODEL"]

    if not cfg.api_key:
        raise ConfigError(
            "No API key. Set DEEPSEEK_API_KEY or add api_key to ~/.deepseek/config.toml"
        )

    return cfg
