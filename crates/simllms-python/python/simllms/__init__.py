"""simllms: one client shape for every LLM provider.

The Rust extension (`simllms._core`) owns the client, the capability Gate,
and the error layers. This package re-exports them and adds `AsyncClient`,
whose calls run on a thread (ADR-0003): concurrency is executor-bound —
in-flight requests scale with the default thread pool, not with the event
loop — but the loop itself never blocks.
"""

from __future__ import annotations

import asyncio
from typing import Any, Sequence

from simllms._core import (
    APIError,
    CapabilityError,
    Client,
    ConfigError,
    Reply,
    SimLLMError,
    TransportError,
)

__all__ = [
    "APIError",
    "AsyncClient",
    "CapabilityError",
    "Client",
    "ConfigError",
    "Reply",
    "SimLLMError",
    "TransportError",
    "__version__",
]

__version__ = "0.1.0"

MessageLike = Any
"""str, or {"role": "system"|"user"|"assistant", "content": str | [block, ...]}.

Blocks: {"type": "text", "text": ...}, {"type": "image_url", "url": ...},
{"type": "image_base64", "data": ..., "mime_type": ...}."""

OptionsLike = dict[str, Any]
"""Keys mirror the Rust `Options`: temperature, top_p, top_k, max_tokens, stop,
seed, presence_penalty, frequency_penalty, reasoning_enabled, reasoning_effort,
response_schema. Provider-specific extras go under `extra`."""


class AsyncClient:
    """Async wrapper over :class:`Client`.

    Each call dispatches to a worker thread via ``asyncio.to_thread``.
    The event loop stays responsive, but total concurrency is bounded by
    the executor, not by the loop.
    """

    def __init__(self, *args: Any, **kwargs: Any) -> None:
        self._sync = Client(*args, **kwargs)

    async def chat(
        self,
        messages: Sequence[MessageLike],
        options: OptionsLike | None = None,
    ) -> Reply:
        return await asyncio.to_thread(self._sync.chat, messages, options)
