"""API client with retry, exponential backoff, and streaming."""

import json
import logging
import random
import time
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

from config import Config
from exceptions import APIError, AuthError, RateLimitError, RelayError

logger = logging.getLogger(__name__)


def _parse_retry_after(e):
    try:
        return float(e.headers.get("Retry-After", ""))
    except (ValueError, AttributeError):
        return None


def _request_with_retry(request, cfg):
    """Send request with exponential-backoff retry. Returns response or raises."""
    last_error = None
    for attempt in range(1, cfg.retry_max_attempts + 1):
        try:
            return urlopen(request, timeout=cfg.request_timeout)
        except HTTPError as e:
            status = e.code
            body = e.read().decode("utf-8", errors="replace")[:500]
            logger.warning(
                "HTTP %d on attempt %d/%d: %s",
                status, attempt, cfg.retry_max_attempts, body,
            )

            if status == 401:
                raise AuthError(
                    "Authentication failed (HTTP 401). Check your API key.",
                    status_code=status, body=body,
                )
            if status == 400:
                raise APIError(
                    f"Bad request (HTTP 400): {body}",
                    status_code=status, body=body,
                )

            if status == 429:
                if attempt < cfg.retry_max_attempts:
                    delay = _parse_retry_after(e) or (
                        cfg.retry_base_delay * (2 ** (attempt - 1))
                    )
                    delay = min(delay + random.uniform(0, 0.5), cfg.retry_max_delay)
                    logger.info("Rate limited, retrying in %.1fs (attempt %d)", delay, attempt)
                    time.sleep(delay)
                    last_error = RateLimitError("Rate limited", status_code=status, body=body)
                    continue
                raise RateLimitError(
                    f"Rate limited after {cfg.retry_max_attempts} retries",
                    status_code=status, body=body,
                )

            if status >= 500:
                if attempt < cfg.retry_max_attempts:
                    delay = min(
                        cfg.retry_base_delay * (2 ** (attempt - 1)) + random.uniform(0, 1),
                        cfg.retry_max_delay,
                    )
                    logger.info("Server error %d, retrying in %.1fs", status, delay)
                    time.sleep(delay)
                    last_error = APIError(
                        f"Server error (HTTP {status})", status_code=status, body=body,
                    )
                    continue
                raise APIError(
                    f"Server error after retries (HTTP {status})",
                    status_code=status, body=body,
                )

            raise APIError(f"API error (HTTP {status})", status_code=status, body=body)

        except URLError as e:
            logger.warning(
                "Network error on attempt %d/%d: %s",
                attempt, cfg.retry_max_attempts, e.reason,
            )
            if attempt < cfg.retry_max_attempts:
                delay = min(
                    cfg.retry_base_delay * (2 ** (attempt - 1)) + random.uniform(0, 1),
                    cfg.retry_max_delay,
                )
                time.sleep(delay)
                last_error = RelayError(f"Network error: {e.reason}")
                continue
            raise RelayError(
                f"Network error after {cfg.retry_max_attempts} retries: {e.reason}"
            ) from e

    raise last_error or RelayError("Request failed after retries")


def stream_chat_completion(messages, tools, cfg):
    """Stream a chat completion, yielding events.

    Yields:
        ("content", str)      — text token
        ("reasoning", str)    — reasoning/chain-of-thought token
        ("tool_call", dict)   — completed tool call {id, name, args}
        ("warning", str)      — non-fatal warning (truncation etc.)
        ("error", str)        — fatal error (stop iterating)
    """
    body = {
        "model": cfg.model,
        "messages": list(messages),
        "stream": True,
        "max_tokens": cfg.max_tokens,
    }
    if tools:
        body["tools"] = tools

    data = json.dumps(body, ensure_ascii=False).encode("utf-8")
    req = Request(
        f"{cfg.base_url}/chat/completions",
        data=data,
        headers={
            "Content-Type": "application/json",
            "Authorization": f"Bearer {cfg.api_key}",
            "Accept": "text/event-stream",
            "User-Agent": "relay-agent/1.0",
        },
    )

    try:
        resp = _request_with_retry(req, cfg)
    except (AuthError, APIError, RelayError) as e:
        yield ("error", str(e))
        return

    buf = b""
    partials = {}

    try:
        for chunk in iter(lambda: resp.read(4096), b""):
            buf += chunk
            while b"\n" in buf:
                line, buf = buf.split(b"\n", 1)
                line = line.decode("utf-8", errors="replace").strip()
                if not line.startswith("data: "):
                    continue
                payload = line[6:].strip()
                if not payload or payload == "[DONE]":
                    break

                try:
                    obj = json.loads(payload)
                except json.JSONDecodeError:
                    logger.warning("Bad SSE JSON: %.100s", payload)
                    continue

                choices = obj.get("choices", [])
                if not choices:
                    continue

                delta = choices[0].get("delta", {})
                finish = choices[0].get("finish_reason")

                for tc in delta.get("tool_calls", []):
                    idx = tc.get("index")
                    if idx is None:
                        continue
                    if idx not in partials:
                        partials[idx] = {"id": "", "name": "", "args": ""}
                    p = partials[idx]
                    if tc.get("id"):
                        p["id"] = tc["id"]
                    fn = tc.get("function", {})
                    if fn.get("name"):
                        p["name"] = fn["name"]
                    if fn.get("arguments"):
                        p["args"] += fn["arguments"]

                if delta.get("content"):
                    yield ("content", delta["content"])
                if delta.get("reasoning_content"):
                    yield ("reasoning", delta["reasoning_content"])
                if finish == "length":
                    yield ("warning", "Response truncated: max_tokens limit reached")

    except Exception as e:
        logger.error("Stream read error: %s", e)
        yield ("error", f"Stream interrupted: {e}")
        return

    for idx in sorted(partials):
        p = partials[idx]
        if p["id"] and p["name"]:
            yield ("tool_call", {"id": p["id"], "name": p["name"], "args": p["args"]})
