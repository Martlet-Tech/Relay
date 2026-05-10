"""Conversation session with automatic context-window management."""

import logging

from config import Config

logger = logging.getLogger(__name__)

_CHARS_PER_TOKEN = 4.5


def _estimate(text):
    return int(len(text) / _CHARS_PER_TOKEN) if text else 0


def _msg_tokens(msg):
    total = _estimate(msg.get("content"))
    if msg.get("role") == "tool":
        total += _estimate(msg.get("content", "")) + 10
    for tc in msg.get("tool_calls", []):
        fn = tc.get("function", {})
        total += _estimate(fn.get("name")) + _estimate(fn.get("arguments"))
    total += 4
    return total


class Session:
    """Conversation history with automatic context-window pruning."""

    def __init__(self, cfg: Config, system_prompt: str = ""):
        self.cfg = cfg
        self._max_tokens = cfg.max_context_tokens - cfg.context_safety_margin
        if system_prompt:
            self.messages: list[dict] = [{"role": "system", "content": system_prompt}]
        else:
            self.messages: list[dict] = []

    def add_user_message(self, content: str):
        self.messages.append({"role": "user", "content": content})

    def add_assistant_message(self, content: str = "", tool_calls: list = None, reasoning: str = None):
        msg = {"role": "assistant"}
        if content:
            msg["content"] = content
        if reasoning:
            msg["reasoning_content"] = reasoning
        if tool_calls:
            msg["tool_calls"] = tool_calls
        self.messages.append(msg)

    def add_tool_result(self, tool_call_id: str, content: str):
        self.messages.append({"role": "tool", "tool_call_id": tool_call_id, "content": content})

    def pop_last_user_message(self):
        for i in range(len(self.messages) - 1, -1, -1):
            if self.messages[i]["role"] == "user":
                return self.messages.pop(i)
        return None

    def total_tokens(self) -> int:
        return sum(_msg_tokens(m) for m in self.messages)

    def ensure_context_fit(self):
        total = self.total_tokens()
        if total <= self._max_tokens:
            return
        logger.info("Context ~%d tokens exceeds limit %d, pruning", total, self._max_tokens)

        last_user = None
        for i in range(len(self.messages) - 1, -1, -1):
            if self.messages[i]["role"] == "user":
                last_user = i
                break
        if last_user is None:
            return

        i = last_user - 1
        while i >= 0 and self.total_tokens() > self._max_tokens:
            if self.messages[i]["role"] == "tool":
                self.messages.pop(i)
                last_user -= 1
            i -= 1

        while len(self.messages) > 4 and self.total_tokens() > self._max_tokens:
            self.messages.pop(0)

        remaining = self.total_tokens()
        if remaining > self._max_tokens:
            logger.warning("Context ~%d tokens still exceeds limit after pruning", remaining)

    def clear(self):
        self.messages.clear()
