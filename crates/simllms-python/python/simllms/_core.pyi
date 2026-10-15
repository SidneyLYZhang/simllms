"""Type hints for `simllms._core` (the pyo3 extension module).

The stub mirrors the Rust surface: Client kwargs, Reply attributes, and the
exception attributes map_error attaches (model/missing/suggestions on
CapabilityError; status/provider_message/raw_body on APIError).
"""

from typing import Any, Sequence


class SimLLMError(Exception): ...


class CapabilityError(SimLLMError):
    model: str
    missing: str
    suggestions: list[str]


class ConfigError(SimLLMError): ...


class APIError(SimLLMError):
    status: int
    provider_message: str
    raw_body: str


class TransportError(SimLLMError): ...


class Reply:
    content: str
    prompt_tokens: int | None
    completion_tokens: int | None
    total_tokens: int | None


class Client:
    def __init__(
        self,
        provider: str,
        model: str,
        api_key: str | None = None,
        *,
        base_url: str | None = None,
        options: dict[str, Any] | None = None,
        capability_overrides: dict[str, Any] | None = None,
        timeout: float | None = None,
    ) -> None: ...

    def chat(
        self,
        messages: Sequence[Any],
        options: dict[str, Any] | None = None,
    ) -> Reply: ...
