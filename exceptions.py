"""Custom exceptions for relay agent."""


class RelayError(Exception):
    """Base relay error."""


class ConfigError(RelayError):
    """Configuration loading failed."""


class APIError(RelayError):
    """API error response."""

    def __init__(self, message, status_code=None, body=None):
        super().__init__(message)
        self.status_code = status_code
        self.body = body


class RateLimitError(APIError):
    """Rate limited by API."""


class AuthError(APIError):
    """Authentication failed (invalid API key, etc.)."""
