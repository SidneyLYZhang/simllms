"""Type hints for `simllms._core` (the pyo3 extension module).

The stub mirrors the Rust surface: Client kwargs, Reply/Chunk/GeneratedImage
attributes, and the exception attributes map_error attaches (model/missing/
suggestions on CapabilityError; status/provider_message/raw_body on APIError).
"""

from typing import Any, Iterator, Sequence


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


class Chunk:
    index: int
    text: str | None
    reasoning: str | None
    finish_reason: str | None
    prompt_tokens: int | None
    completion_tokens: int | None
    total_tokens: int | None


class GeneratedImage:
    data: bytes | None
    url: str | None
    mime_type: str | None
    revised_prompt: str | None


class ChatStream(Iterator[Chunk]):
    def __iter__(self) -> ChatStream: ...
    def __next__(self) -> Chunk: ...


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

    def chat_stream(
        self,
        messages: Sequence[Any],
        options: dict[str, Any] | None = None,
    ) -> ChatStream: ...

    def generate_image(
        self,
        prompt: str,
        *,
        size: str | None = None,
        aspect_ratio: str | None = None,
        n: int | None = None,
        negative_prompt: str | None = None,
        seed: int | None = None,
        quality: str | None = None,
        output_format: str | None = None,
        background: str | None = None,
        poll_interval: float | None = None,
    ) -> list[GeneratedImage]: ...

    def debug_request(
        self,
        messages: Sequence[Any],
        options: dict[str, Any] | None = None,
    ) -> str: ...
