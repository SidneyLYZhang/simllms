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
from collections.abc import AsyncIterator, Iterator, Sequence
from typing import Any

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
    "CapabilityError",
    "Chunk",
    "Client",
    "ConfigError",
    "GeneratedImage",
    "LOGGER_NAME",
    "Reply",
    "SimLLMError",
    "TransportError",
    "__version__",
]

__version__ = "0.1.0"

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
    ) -> None:
        self._core = _core.Client(
            provider,
            model,
            api_key,
            base_url=base_url,
            options=options,
            capability_overrides=capability_overrides,
            timeout=timeout,
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
