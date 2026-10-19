"""simllms: one client shape for every LLM provider.

The Rust extension (`simllms._core`) owns the client, the capability Gate,
and the error layers. This package re-exports them, adds `AsyncClient`
(ADR-0003: calls run on a worker thread — concurrency is executor-bound,
the event loop never blocks), and logs to the fixed logger name
``"simllms"`` (:data:`LOGGER_NAME`) via standard ``logging`` — attach a
handler to that name to hear it. No credentials are ever logged.
"""

from __future__ import annotations

import asyncio
import logging
from collections.abc import AsyncIterator, Iterator, Mapping, Sequence
from dataclasses import dataclass
from typing import Any, Literal, cast

from simllms import _core
from simllms._core import (
    APIError,
    CapabilityError,
    Chunk,
    ConfigError,
    GeneratedImage,
    Reply,
    SimLLMError,
    TransportError,
)

__all__ = [
    "APIError",
    "AsyncClient",
    "Auth",
    "CapabilityError",
    "Chunk",
    "Client",
    "ConfigError",
    "DashScopeRegion",
    "GeneratedImage",
    "LOGGER_NAME",
    "Reply",
    "RetryPolicy",
    "SimLLMError",
    "TransportError",
    "Warning",
    "__version__",
]

__version__ = _core.__version__

LOGGER_NAME = "simllms"
logger = logging.getLogger(LOGGER_NAME)

MessageLike = Any
"""str, or {"role": "system"|"user"|"assistant", "content": str | [block, ...]}.

Blocks: {"type": "text", "text": ...}, {"type": "image_url", "url": ...},
{"type": "image_base64", "data": ..., "mime_type": ...}."""

OptionsLike = dict[str, Any]
"""Keys mirror the Rust `Options`: temperature, top_p, top_k, max_tokens, stop,
seed, presence_penalty, frequency_penalty, reasoning_enabled, reasoning_effort,
response_schema. Provider-specific extras go under `extra`."""

_DONE = object()

WarningKind = Literal["unknown_model", "deprecated_model"]


@dataclass(frozen=True)
class Auth:
    """The credential's header shape.

    :meth:`Auth.bearer` (what ``auth=None`` gives you, except on MiMo) sends
    ``Authorization: Bearer <key>``; :meth:`Auth.header` sends the key in a
    named header — for gateways that want ``api-key:`` or ``x-api-key:``.
    """

    header_name: str | None = None
    header_value: str | None = None

    @staticmethod
    def bearer() -> Auth:
        return Auth()

    @staticmethod
    def header(name: str, value: str) -> Auth:
        if not name:
            raise ValueError("`Auth.header` needs a header name")
        return Auth(name, value)


@dataclass(frozen=True)
class DashScopeRegion:
    """DashScope's host selection.

    :meth:`DashScopeRegion.cn` is the shared Beijing host (the default),
    :meth:`DashScopeRegion.singapore` the shared international one, and
    :meth:`DashScopeRegion.workspace` a dedicated workspace endpoint. An
    explicit ``base_url`` outranks any region.
    """

    kind: Literal["cn", "singapore", "workspace"]
    id: str | None = None
    region: str | None = None

    @staticmethod
    def cn() -> DashScopeRegion:
        return DashScopeRegion("cn")

    @staticmethod
    def singapore() -> DashScopeRegion:
        return DashScopeRegion("singapore")

    @staticmethod
    def workspace(id: str, region: str) -> DashScopeRegion:
        return DashScopeRegion("workspace", id=id, region=region)


@dataclass(frozen=True)
class RetryPolicy:
    """Attempt-count policy for chat and streaming calls.

    ``max_attempts`` counts total sends, not retries; the default is 3.
    :meth:`RetryPolicy.disabled` turns retrying off.
    """

    max_attempts: int = 3

    @staticmethod
    def disabled() -> RetryPolicy:
        return RetryPolicy(max_attempts=1)


@dataclass(frozen=True)
class Warning:
    """One of the Gate's advisory answers about a request.

    ``kind`` is ``"unknown_model"`` when the Model Id is not in the catalog
    (``provider`` names which catalog it was missed in) or
    ``"deprecated_model"`` when it is retired. Both pass through at call
    time — a warning is advice, never a refusal.
    """

    kind: WarningKind
    provider: str | None
    model: str


class Client:
    """Annotated surface over the Rust core.

    Emits to the fixed logger :data:`LOGGER_NAME` (``"simllms"``): client
    creation, chat calls, and stream starts at DEBUG, nothing above INFO.
    """

    def __init__(
        self,
        provider: str,
        model: str,
        api_key: str | None = None,
        *,
        base_url: str | None = None,
        options: OptionsLike | None = None,
        capability_overrides: dict[str, Any] | None = None,
        timeout: float | None = None,
        auth: Auth | None = None,
        dashscope_region: DashScopeRegion | None = None,
        zhipu_international: bool = False,
        default_headers: Mapping[str, str] | None = None,
        retry: RetryPolicy | None = None,
        stream_timeouts: tuple[float, float] | None = None,
    ) -> None:
        auth_kwarg: tuple[str, ...] | None
        if auth is None:
            auth_kwarg = None
        elif auth.header_name is None:
            auth_kwarg = ("bearer",)
        else:
            auth_kwarg = ("header", auth.header_name, auth.header_value or "")
        region_kwarg: str | tuple[str, str, str] | None
        if dashscope_region is None:
            region_kwarg = None
        elif dashscope_region.kind == "workspace":
            region_kwarg = (
                "workspace",
                dashscope_region.id or "",
                dashscope_region.region or "",
            )
        else:
            region_kwarg = dashscope_region.kind
        retry_kwarg = (
            (retry.max_attempts > 1, retry.max_attempts) if retry else None
        )
        self._core = _core.Client(
            provider,
            model,
            api_key,
            base_url=base_url,
            options=options,
            capability_overrides=capability_overrides,
            timeout=timeout,
            auth=auth_kwarg,
            dashscope_region=region_kwarg,
            zhipu_international=zhipu_international,
            default_headers=dict(default_headers) if default_headers else None,
            retry=retry_kwarg,
            stream_timeouts=stream_timeouts,
        )
        logger.debug("client created: %s/%s", provider, model)

    def chat(
        self,
        messages: Sequence[MessageLike],
        options: OptionsLike | None = None,
    ) -> Reply:
        logger.debug("chat: %d message(s)", len(messages))
        reply = self._core.chat(messages, options)
        logger.debug("chat done: %d character(s)", len(reply.content))
        return reply

    def chat_stream(
        self,
        messages: Sequence[MessageLike],
        options: OptionsLike | None = None,
    ) -> Iterator[Chunk]:
        """Plain iterator of normalized Chunks. Each `next()` blocks the
        calling thread on one SSE event; use AsyncClient to avoid that."""
        logger.debug("chat_stream: %d message(s)", len(messages))
        return self._core.chat_stream(messages, options)

    def generate_image(self, prompt: str, **kwargs: Any) -> list[GeneratedImage]:
        """kwargs: size, aspect_ratio, n, negative_prompt, seed, quality,
        output_format, background, poll_interval (seconds)."""
        logger.debug("generate_image: %d character(s) prompt", len(prompt))
        return self._core.generate_image(prompt, **kwargs)

    def debug_request(
        self,
        messages: Sequence[MessageLike],
        options: OptionsLike | None = None,
    ) -> str:
        """The exact request this client would send — credential-free."""
        return self._core.debug_request(messages, options)

    def warnings(
        self,
        messages: Sequence[MessageLike],
        options: OptionsLike | None = None,
    ) -> list[Warning]:
        """The Gate's structured warnings for this request, answered from the
        catalog alone: an unknown Model Id (:attr:`Warning.kind` is
        ``"unknown_model"``) and a deprecated one (``"deprecated_model"``)
        pass through at call time — this makes them visible before you pay.

        The _core pyclass carries ``kind`` as a plain str because it crosses
        the extension boundary; only the two kinds below ever exist (the
        Rust mapping refuses any other), hence the cast.
        """
        return [
            Warning(kind=cast(WarningKind, core.kind), provider=core.provider, model=core.model)
            for core in self._core.warnings(messages, options)
        ]

    def refresh_models(self) -> list[str]:
        """Consult the provider's own model listing (blocking). Only
        DeepSeek, Moonshot, DashScope and OpenRouter publish one; other
        providers raise ConfigError, and the catalog stays authoritative."""
        return self._core.refresh_models()


class AsyncClient:
    """Async wrapper over :class:`Client`.

    Each call dispatches to a worker thread via ``asyncio.to_thread``.
    The event loop stays responsive, but total concurrency is bounded by
    the executor, not by the loop. `chat_stream` is an async generator:
    each chunk is fetched on a worker thread, so the loop stays free even
    mid-stream.
    """

    def __init__(self, *args: Any, **kwargs: Any) -> None:
        self._sync = Client(*args, **kwargs)

    async def chat(
        self,
        messages: Sequence[MessageLike],
        options: OptionsLike | None = None,
    ) -> Reply:
        return await asyncio.to_thread(self._sync.chat, messages, options)

    async def chat_stream(
        self,
        messages: Sequence[MessageLike],
        options: OptionsLike | None = None,
    ) -> AsyncIterator[Chunk]:
        stream = self._sync.chat_stream(messages, options)
        iterator = iter(stream)
        while True:
            chunk = await asyncio.to_thread(next, iterator, _DONE)
            if chunk is _DONE or not isinstance(chunk, Chunk):
                return
            yield chunk

    async def generate_image(self, prompt: str, **kwargs: Any) -> list[GeneratedImage]:
        return await asyncio.to_thread(self._sync.generate_image, prompt, **kwargs)

    async def refresh_models(self) -> list[str]:
        return await asyncio.to_thread(self._sync.refresh_models)
