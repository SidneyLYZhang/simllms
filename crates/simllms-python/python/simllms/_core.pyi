"""Type hints for `simllms._core` (the pyo3 extension module).

The stub mirrors the Rust surface: Client kwargs, Reply/Chunk/GeneratedImage/
Warning/Capabilities attributes, the exception attributes map_error attaches
(model/missing/suggestions on CapabilityError; status/provider_message/
raw_body on APIError), and the module-level catalog queries. The primitive
shapes of `auth`, `dashscope_region` and `retry` are what the pure-Python
layer sends after decomposing its dataclasses: `("bearer",)` or a
`("header", name, value)` triple; `"cn"`, `"singapore"` or a
`("workspace", id, region)` triple; an `(enabled, max_attempts)` pair. The
pure-Python layer re-exports `Capabilities`' fields as a frozen dataclass;
this pyclass is the wire form, `capability_overrides` here stays a dict of
only-set fields because the typed object never crosses this boundary.
"""

from typing import Any, Iterator, Sequence

__version__: str


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


class Warning:
    kind: str
    provider: str | None
    model: str


class Capabilities:
    source: str
    input_modalities: list[str]
    structured_output: bool
    tool_call: bool
    image_generation: bool
    reasoning: str
    reasoning_levels: list[str]
    context_window: int | None
    max_output_tokens: int | None
    status: str


def capabilities(provider: str, model: str) -> Capabilities | None: ...


def models_with(
    provider: str, filters: dict[str, bool] | None = None
) -> list[str]: ...


CAPABILITY_FILTERS: tuple[str, ...]


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
        auth: tuple[str, ...] | None = None,
        dashscope_region: str | tuple[str, str, str] | None = None,
        zhipu_international: bool = False,
        default_headers: dict[str, str] | None = None,
        retry: tuple[bool, int] | None = None,
        stream_timeouts: tuple[float, float] | None = None,
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

    def warnings(
        self,
        messages: Sequence[Any],
        options: dict[str, Any] | None = None,
    ) -> list[Warning]: ...

    def refresh_models(self) -> list[str]: ...
