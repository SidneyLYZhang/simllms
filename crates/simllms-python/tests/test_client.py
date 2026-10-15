"""Tests for the simllms Python bindings, run against a local mock gateway.

The mock server speaks the OpenAI-compatible wire shape that the Rust core
emits and consumes; the Python layer must behave exactly as the Rust tests
assert, and the exception attributes must carry the provider's evidence
(`raw_body`) through untouched.
"""

from __future__ import annotations

import asyncio
import json
import threading
import time
from collections.abc import Iterator
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any, cast

import pytest

import simllms
from simllms import (
    APIError,
    CapabilityError,
    Client,
    ConfigError,
    SimLLMError,
    TransportError,
)

SUCCESS_BODY = {
    "id": "chatcmpl-1",
    "object": "chat.completion",
    "choices": [
        {
            "index": 0,
            "finish_reason": "stop",
            "message": {"role": "assistant", "content": "hello from the mock"},
        }
    ],
    "usage": {"prompt_tokens": 3, "completion_tokens": 5, "total_tokens": 8},
}

ERROR_BODY = {
    "error": {"message": "invalid api key", "type": "auth_error", "code": "401"}
}


class MockHandler(BaseHTTPRequestHandler):
    def log_message(self, format: str, *args: Any) -> None:  # noqa: A002
        pass

    def do_POST(self) -> None:
        behavior = self.server.behavior  # type: ignore[attr-defined]
        behavior["request_log"].append(self._read_json())

        for prefix, spec in behavior["routes"].items():
            if self.path.startswith(prefix):
                spec["handler"](self)
                return

        self._respond(200, SUCCESS_BODY)

    def _read_json(self) -> Any:
        length = int(self.headers.get("Content-Length", "0"))
        raw = self.rfile.read(length)
        return json.loads(raw) if raw else None

    def _respond(self, status: int, body: Any, delay: float = 0.0) -> None:
        if delay:
            time.sleep(delay)
        payload = json.dumps(body).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


class MockGateway:
    def __init__(self) -> None:
        self.request_log: list[Any] = []
        self.routes: dict[str, dict[str, Any]] = {}
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), MockHandler)
        self.server.behavior = {  # type: ignore[attr-defined]
            "request_log": self.request_log,
            "routes": self.routes,
        }
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    @property
    def base_url(self) -> str:
        address = cast(tuple[str, int], self.server.server_address)
        return f"http://{address[0]}:{address[1]}/v1"

    def route(self, prefix: str, handler: Any) -> None:
        self.routes[prefix] = {"handler": handler}

    def close(self) -> None:
        self.server.shutdown()
        self.server.server_close()


@pytest.fixture
def gateway() -> Iterator[MockGateway]:
    gw = MockGateway()
    yield gw
    gw.close()


def make_client(gateway: MockGateway, **kwargs: Any) -> Client:
    return Client(
        "openrouter",
        "test-model",
        api_key="sk-test",
        base_url=gateway.base_url,
        **kwargs,
    )


def test_chat_returns_reply(gateway: MockGateway) -> None:
    client = make_client(gateway)
    reply = client.chat(["hi", {"role": "system", "content": "be brief"}])

    assert reply.content == "hello from the mock"
    assert reply.prompt_tokens == 3
    assert reply.completion_tokens == 5
    assert reply.total_tokens == 8


def test_chat_matches_rust_wire_shape(gateway: MockGateway) -> None:
    client = make_client(gateway)
    client.chat(["hi"], options={"temperature": 0.5, "max_tokens": 32})

    body = gateway.request_log[-1]
    assert body["model"] == "test-model"
    assert body["messages"] == [{"role": "user", "content": "hi"}]
    assert body["temperature"] == 0.5
    assert body["max_tokens"] == 32
    assert "stream" not in body


def test_api_error_carries_raw_body(gateway: MockGateway) -> None:
    def unauthorized(handler: MockHandler) -> None:
        handler._respond(401, ERROR_BODY)

    gateway.route("/v1/unauth", unauthorized)
    client = Client(
        "openrouter",
        "test-model",
        api_key="sk-test",
        base_url=gateway.base_url + "/unauth",
    )

    with pytest.raises(APIError) as excinfo:
        client.chat(["hi"])

    assert excinfo.value.status == 401
    assert excinfo.value.provider_message == "invalid api key"
    assert json.loads(excinfo.value.raw_body) == ERROR_BODY
    assert isinstance(excinfo.value, SimLLMError)


def test_api_error_status_zero_on_2xx_without_choices(gateway: MockGateway) -> None:
    def garbage(handler: MockHandler) -> None:
        handler._respond(200, {"note": "no choices here"})

    gateway.route("/v1/garbage", garbage)
    client = Client(
        "openrouter",
        "test-model",
        api_key="sk-test",
        base_url=gateway.base_url + "/garbage",
    )

    with pytest.raises(APIError) as excinfo:
        client.chat(["hi"])
    # A 2xx body without choices is a provider-reported failure mid-flow:
    # parse_reply reports it with the real status (200), raw_body intact.
    assert excinfo.value.status == 200
    assert "no choices here" in excinfo.value.raw_body


def test_capability_error_refuses_locally() -> None:
    # deepseek-v4-pro is Known in the catalog as text-only: the gate must
    # refuse an image message before any network call (base_url points at a
    # dead port so a live request would surface as TransportError instead).
    client = Client(
        "deepseek",
        "deepseek-v4-pro",
        api_key="sk-test",
        base_url="http://127.0.0.1:1",
    )
    messages = [
        {
            "role": "user",
            "content": [
                {"type": "text", "text": "what is this?"},
                {"type": "image_url", "url": "https://example.com/cat.png"},
            ],
        }
    ]

    with pytest.raises(CapabilityError) as excinfo:
        client.chat(messages)

    assert excinfo.value.model == "deepseek-v4-pro"
    assert excinfo.value.missing == "ImageInput"
    assert isinstance(excinfo.value, SimLLMError)


def test_config_error_custom_without_base_url() -> None:
    client = Client("custom", "my-model", api_key="sk-test")
    with pytest.raises(ConfigError):
        client.chat(["hi"])


def test_transport_error_invalid_base_url() -> None:
    client = Client(
        "openrouter",
        "test-model",
        api_key="sk-test",
        base_url="not a url at all",
    )
    with pytest.raises(TransportError):
        client.chat(["hi"])


def test_api_key_from_env(gateway: MockGateway, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("OPENROUTER_API_KEY", "sk-from-env")
    client = Client(
        "openrouter", "test-model", base_url=gateway.base_url, timeout=10.0
    )
    reply = client.chat(["hi"])
    assert reply.content == "hello from the mock"


def test_unknown_option_is_type_error(gateway: MockGateway) -> None:
    client = make_client(gateway)
    with pytest.raises(TypeError):
        client.chat(["hi"], options={"bogus_option": 1})


def test_extra_options_reach_the_wire(gateway: MockGateway) -> None:
    client = make_client(gateway)
    client.chat(["hi"], options={"extra": {"enable_search": True}})

    body = gateway.request_log[-1]
    assert body["enable_search"] is True


def test_async_chat_does_not_block_event_loop(gateway: MockGateway) -> None:
    def slow(handler: MockHandler) -> None:
        handler._respond(200, SUCCESS_BODY, delay=1.0)

    gateway.route("/v1/slow", slow)
    client = simllms.AsyncClient(
        "openrouter",
        "test-model",
        api_key="sk-test",
        base_url=gateway.base_url + "/slow",
    )

    async def main() -> int:
        ticks = 0

        async def heartbeat() -> None:
            nonlocal ticks
            while True:
                await asyncio.sleep(0.05)
                ticks += 1

        hb = asyncio.create_task(heartbeat())
        try:
            reply = await client.chat(["hi"])
            assert reply.content == "hello from the mock"
        finally:
            hb.cancel()
        return ticks

    ticks = asyncio.run(main())
    # A 1.0s slow call must leave the loop free to tick far more often than
    # a blocking call would allow (a blocked loop yields exactly 0).
    assert ticks >= 10, f"event loop ticked only {ticks} times during chat"


def test_async_concurrent_chats(gateway: MockGateway) -> None:
    client = simllms.AsyncClient(
        "openrouter", "test-model", api_key="sk-test", base_url=gateway.base_url
    )

    async def main() -> list[str]:
        return [r.content for r in await asyncio.gather(*[client.chat(["hi"]) for _ in range(5)])]

    contents = asyncio.run(main())
    assert contents == ["hello from the mock"] * 5
