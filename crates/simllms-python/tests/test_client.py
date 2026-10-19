"""Tests for the simllms Python bindings, run against a local mock gateway.

The mock server speaks the OpenAI-compatible wire shape that the Rust core
emits and consumes; the Python layer must behave exactly as the Rust tests
assert, and the exception attributes must carry the provider's evidence
(`raw_body`) through untouched.
"""

from __future__ import annotations

import asyncio
import base64
import json
import logging
import threading
import time
from collections.abc import Iterator
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any, cast

import pytest

import simllms
from simllms import (
    APIError,
    AsyncClient,
    Auth,
    CapabilityError,
    Client,
    ConfigError,
    DashScopeRegion,
    RetryPolicy,
    SimLLMError,
    TransportError,
)
from simllms import LOGGER_NAME

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
        behavior["headers_log"].append(
            {k.lower(): v for k, v in self.headers.items()}
        )
        self._route(behavior)

    def do_GET(self) -> None:
        behavior = self.server.behavior  # type: ignore[attr-defined]
        behavior["request_log"].append(None)
        self._route(behavior)

    def _route(self, behavior: dict[str, Any]) -> None:
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
        self.headers_log: list[dict[str, str]] = []
        self.routes: dict[str, dict[str, Any]] = {}
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), MockHandler)
        self.server.behavior = {  # type: ignore[attr-defined]
            "request_log": self.request_log,
            "headers_log": self.headers_log,
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


SSE_BODY = (
    'data: {"choices":[{"index":0,"delta":{"role":"assistant","content":"Hel"}}]}\n\n'
    'data: {"choices":[{"index":0,"delta":{"content":"lo"}}]}\n\n'
    'data: {"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],'
    '"usage":{"prompt_tokens":4,"completion_tokens":2,"total_tokens":6}}\n\n'
    "data: [DONE]\n\n"
)


def test_sync_stream_yields_normalized_chunks(gateway: MockGateway) -> None:
    def sse(handler: MockHandler) -> None:
        payload = SSE_BODY.encode("utf-8")
        handler.send_response(200)
        handler.send_header("Content-Type", "text/event-stream")
        handler.send_header("Content-Length", str(len(payload)))
        handler.end_headers()
        handler.wfile.write(payload)

    gateway.route("/v1/stream", sse)
    client = Client(
        "openrouter",
        "test-model",
        api_key="sk-test",
        base_url=gateway.base_url + "/stream",
    )

    chunks = list(client.chat_stream(["hi"]))

    assert [c.text for c in chunks if c.text is not None] == ["Hel", "lo"]
    final = chunks[-1]
    assert final.finish_reason == "stop"
    assert final.total_tokens == 6
    # The wire request carried the streaming opt-ins.
    body = gateway.request_log[-1]
    assert body["stream"] is True
    assert body["stream_options"] == {"include_usage": True}


def test_async_stream_yields_normalized_chunks(gateway: MockGateway) -> None:
    def sse_slow(handler: MockHandler) -> None:
        head, rest = SSE_BODY.split("\n\n", 1)  # first event, then the rest
        payload1 = (head + "\n\n").encode("utf-8")
        handler.send_response(200)
        handler.send_header("Content-Type", "text/event-stream")
        handler.end_headers()
        handler.wfile.write(payload1)
        handler.wfile.flush()
        time.sleep(0.3)  # make the loop demonstrably free mid-stream
        handler.wfile.write(rest.encode("utf-8"))
        handler.wfile.flush()

    gateway.route("/v1/stream", sse_slow)
    client = simllms.AsyncClient(
        "openrouter",
        "test-model",
        api_key="sk-test",
        base_url=gateway.base_url + "/stream",
    )

    async def main() -> tuple[list[str], int]:
        texts: list[str] = []
        ticks = 0

        async def heartbeat() -> None:
            nonlocal ticks
            while True:
                await asyncio.sleep(0.05)
                ticks += 1

        hb = asyncio.create_task(heartbeat())
        try:
            async for chunk in client.chat_stream(["hi"]):
                if chunk.text is not None:
                    texts.append(chunk.text)
        finally:
            hb.cancel()
        return texts, ticks

    texts, ticks = asyncio.run(main())
    assert texts == ["Hel", "lo"]
    # The second event arrives 0.3s late; each chunk is fetched on a worker
    # thread, so the loop must keep ticking through the wait.
    assert ticks >= 3, f"event loop ticked only {ticks} times during stream"


def test_structured_output_refusal_surfaces_locally() -> None:
    # User-declared override tightens the model to no structured output: the
    # Gate refuses before any network call (dead base_url would surface any
    # live request as TransportError, never CapabilityError).
    client = Client(
        "deepseek",
        "deepseek-v4-pro",
        api_key="sk-test",
        base_url="http://127.0.0.1:1",
        capability_overrides={"structured_output": False},
    )
    schema = {"type": "object", "properties": {"answer": {"type": "string"}}}

    with pytest.raises(CapabilityError) as excinfo:
        client.chat(["hi"], options={"response_schema": schema})

    assert excinfo.value.missing == "StructuredOutput"
    assert excinfo.value.model == "deepseek-v4-pro"


def test_structured_output_reaches_the_wire_native_only(gateway: MockGateway) -> None:
    client = make_client(gateway)
    schema = {"type": "object", "properties": {"answer": {"type": "string"}}}
    client.chat(["hi"], options={"response_schema": schema})

    body = gateway.request_log[-1]
    # ADR-0004: native response_format only, never schema-as-prompt-text.
    assert body["response_format"] == {
        "type": "json_schema",
        "json_schema": {"name": "response", "strict": True, "schema": schema},
    }
    assert json.dumps(schema) not in body["messages"][0]["content"]


def test_generate_image_zhipu_url_shape(gateway: MockGateway) -> None:
    def images(handler: MockHandler) -> None:
        handler._respond(
            200,
            {
                "data": [
                    {
                        "url": "https://img.example/cat.png",
                        "revised_prompt": "a tabby cat",
                    }
                ]
            },
        )

    gateway.route("/v1/images", images)
    client = Client(
        "zhipu",
        "cogview-4",
        api_key="sk-test",
        base_url=gateway.base_url,
    )

    result = client.generate_image("a cat", size="1024x1024")

    assert len(result) == 1
    assert result[0].url == "https://img.example/cat.png"
    assert result[0].data is None  # bytes-or-URL invariant
    assert result[0].revised_prompt == "a tabby cat"
    assert gateway.request_log[-1]["size"] == "1024x1024"


def test_generate_image_openrouter_b64_shape(gateway: MockGateway) -> None:
    png = b"\x89PNG fake bytes"

    def images(handler: MockHandler) -> None:
        handler._respond(
            200,
            {"data": [{"b64_json": base64.b64encode(png).decode("ascii")}]},
        )

    gateway.route("/v1/images", images)
    client = make_client(gateway)

    result = client.generate_image("a cat")

    assert len(result) == 1
    assert result[0].data == png
    assert result[0].url is None


def test_generate_image_dashscope_polling(gateway: MockGateway) -> None:
    def submit(handler: MockHandler) -> None:
        handler._respond(200, {"output": {"task_id": "t-9", "task_status": "PENDING"}})

    polls = {"count": 0}

    def poll(handler: MockHandler) -> None:
        polls["count"] += 1
        if polls["count"] < 3:
            handler._respond(200, {"output": {"task_status": "PENDING"}})
        else:
            handler._respond(
                200,
                {
                    "output": {
                        "task_status": "SUCCEEDED",
                        "results": [{"url": "https://img.example/q.png"}],
                    }
                },
            )

    gateway.route("/v1/api/v1/services/aigc/text2image/image-synthesis", submit)
    gateway.route("/v1/api/v1/tasks/", poll)
    client = Client(
        "dashscope",
        "wanx2.1-t2i-turbo",
        api_key="sk-test",
        base_url=gateway.base_url + "/compatible-mode/v1",
    )

    result = client.generate_image("a cat", poll_interval=0.02)

    assert result[0].url == "https://img.example/q.png"
    assert polls["count"] == 3


def test_async_generate_image_does_not_block_event_loop(gateway: MockGateway) -> None:
    polls = {"count": 0}

    def submit(handler: MockHandler) -> None:
        handler._respond(200, {"output": {"task_id": "t-10", "task_status": "PENDING"}})

    def poll(handler: MockHandler) -> None:
        polls["count"] += 1
        if polls["count"] < 4:
            time.sleep(0.2)  # slow poll: the loop must stay free regardless
            handler._respond(200, {"output": {"task_status": "PENDING"}})
        else:
            handler._respond(
                200,
                {
                    "output": {
                        "task_status": "SUCCEEDED",
                        "results": [{"url": "https://img.example/q.png"}],
                    }
                },
            )

    gateway.route("/v1/api/v1/services/aigc/text2image/image-synthesis", submit)
    gateway.route("/v1/api/v1/tasks/", poll)
    client = simllms.AsyncClient(
        "dashscope",
        "wanx2.1-t2i-turbo",
        api_key="sk-test",
        base_url=gateway.base_url + "/compatible-mode/v1",
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
            result = await client.generate_image("a cat", poll_interval=0.02)
            assert result[0].url == "https://img.example/q.png"
        finally:
            hb.cancel()
        return ticks

    ticks = asyncio.run(main())
    assert ticks >= 5, f"event loop ticked only {ticks} times during image polling"


def test_debug_request_is_credential_free(gateway: MockGateway) -> None:
    client = make_client(gateway)
    debug = client.debug_request(["hi"], options={"max_tokens": 32})

    assert "test-model" in debug
    assert "max_tokens" in debug
    assert "sk-test" not in debug


def test_logging_under_fixed_logger_name(
    gateway: MockGateway, caplog: pytest.LogCaptureFixture
) -> None:
    with caplog.at_level(logging.DEBUG, logger=LOGGER_NAME):
        client = make_client(gateway)
        client.chat(["hi"])

    simllms_records = [r for r in caplog.records if r.name == LOGGER_NAME]
    assert simllms_records, "no log records emitted under the simllms logger"
    assert any("client created" in r.getMessage() for r in simllms_records)
    assert any("chat" in r.getMessage() for r in simllms_records)
    assert all("sk-test" not in r.getMessage() for r in simllms_records)


# --- Configuration parity (spec #23 / ticket #24) ---------------------------
# The mock sees headers but never the host a region would pick: an explicit
# base_url wins over a region, so every test here stays on the mock gateway.


def test_default_auth_is_bearer_on_the_wire(gateway: MockGateway) -> None:
    client = make_client(gateway)
    client.chat(["hi"])

    headers = gateway.headers_log[-1]
    assert headers["authorization"] == "Bearer sk-test"


def test_auth_header_shape_reaches_the_wire(gateway: MockGateway) -> None:
    client = make_client(gateway, auth=Auth.header("x-api-key", "sk-x"))
    client.chat(["hi"])

    headers = gateway.headers_log[-1]
    assert headers["x-api-key"] == "sk-x"
    assert "authorization" not in headers


def test_auth_bearer_is_explicitly_selectable(gateway: MockGateway) -> None:
    client = make_client(gateway, auth=Auth.bearer())
    client.chat(["hi"])

    assert gateway.headers_log[-1]["authorization"] == "Bearer sk-test"


def test_default_headers_ride_on_the_request(gateway: MockGateway) -> None:
    client = make_client(gateway, default_headers={"X-Tenant": "acme"})
    client.chat(["hi"])

    assert gateway.headers_log[-1]["x-tenant"] == "acme"


def test_explicit_base_url_wins_over_the_region_host(gateway: MockGateway) -> None:
    # Host selection itself (cn/singapore/workspace/zhipu international) is
    # proven in the Rust core's own tests; hermetically, the binding proves
    # the region kwarg is applied by exercising the precedence an explicit
    # base_url keeps over it.
    client = Client(
        "dashscope",
        "test-model",
        api_key="sk-test",
        base_url=gateway.base_url,
        dashscope_region=DashScopeRegion.singapore(),
    )
    reply = client.chat(["hi"])

    assert reply.content == "hello from the mock"


def test_explicit_base_url_wins_over_zhipu_international(
    gateway: MockGateway,
) -> None:
    client = Client(
        "zhipu",
        "test-model",
        api_key="sk-test",
        base_url=gateway.base_url,
        zhipu_international=True,
    )
    reply = client.chat(["hi"])

    assert reply.content == "hello from the mock"


def _capturing_core(captured: dict[str, Any]) -> type:
    """A stand-in for `_core.Client` that records the pure-Python layer's kwargs."""

    class FakeCore:
        def __init__(self, *args: Any, **kwargs: Any) -> None:
            captured.update(kwargs)

    return FakeCore


def test_region_auth_retry_and_header_kwargs_translate_to_core(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured: dict[str, Any] = {}
    monkeypatch.setattr(simllms._core, "Client", _capturing_core(captured))
    simllms.Client(
        "dashscope",
        "test-model",
        api_key="sk-test",
        auth=Auth.header("x-api-key", "sk-x"),
        dashscope_region=DashScopeRegion.workspace("llm-x", "cn-beijing"),
        retry=RetryPolicy(max_attempts=5),
        stream_timeouts=(1.0, 2.0),
        default_headers={"X-Tenant": "acme"},
    )

    assert captured["auth"] == ("header", "x-api-key", "sk-x")
    assert captured["dashscope_region"] == ("workspace", "llm-x", "cn-beijing")
    assert captured["retry"] == (True, 5)
    assert captured["stream_timeouts"] == (1.0, 2.0)
    assert captured["default_headers"] == {"X-Tenant": "acme"}
    assert captured["zhipu_international"] is False


def test_bearer_and_region_factories_translate_to_core(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured: dict[str, Any] = {}
    monkeypatch.setattr(simllms._core, "Client", _capturing_core(captured))
    simllms.Client(
        "dashscope",
        "test-model",
        api_key="sk-test",
        auth=Auth.bearer(),
        dashscope_region=DashScopeRegion.cn(),
    )

    assert captured["auth"] == ("bearer",)
    assert captured["dashscope_region"] == "cn"


def test_retry_policy_retries_then_succeeds(gateway: MockGateway) -> None:
    state = {"count": 0}

    def flaky(handler: MockHandler) -> None:
        state["count"] += 1
        if state["count"] <= 2:
            handler._respond(500, {"error": {"message": "provider hiccup"}})
        else:
            handler._respond(200, SUCCESS_BODY)

    gateway.route("/v1/flaky", flaky)
    client = Client(
        "openrouter",
        "test-model",
        api_key="sk-test",
        base_url=gateway.base_url + "/flaky",
        retry=RetryPolicy(max_attempts=3),
    )

    reply = client.chat(["hi"])

    assert reply.content == "hello from the mock"
    assert len(gateway.request_log) == 3


def test_retry_disabled_fails_on_the_first_attempt(gateway: MockGateway) -> None:
    def broken(handler: MockHandler) -> None:
        handler._respond(500, {"error": {"message": "provider hiccup"}})

    gateway.route("/v1/broken", broken)
    client = Client(
        "openrouter",
        "test-model",
        api_key="sk-test",
        base_url=gateway.base_url + "/broken",
        retry=RetryPolicy.disabled(),
    )

    with pytest.raises(APIError) as excinfo:
        client.chat(["hi"])

    assert excinfo.value.status == 500
    assert len(gateway.request_log) == 1


def _sse_handler(handler: MockHandler) -> None:
    payload = SSE_BODY.encode("utf-8")
    handler.send_response(200)
    handler.send_header("Content-Type", "text/event-stream")
    handler.send_header("Content-Length", str(len(payload)))
    handler.end_headers()
    handler.wfile.write(payload)


def test_stream_first_token_timeout_surfaces_transport_error(
    gateway: MockGateway,
) -> None:
    def sse_asleep(handler: MockHandler) -> None:
        # The head arrives at once; the body stalls past the first-token
        # timeout — the same shape the Rust core's own timeout test uses.
        payload = SSE_BODY.encode("utf-8")
        handler.send_response(200)
        handler.send_header("Content-Type", "text/event-stream")
        handler.send_header("Content-Length", str(len(payload)))
        handler.end_headers()
        time.sleep(1.0)
        handler.wfile.write(payload)

    gateway.route("/v1/asleep", sse_asleep)
    client = Client(
        "openrouter",
        "test-model",
        api_key="sk-test",
        base_url=gateway.base_url + "/asleep",
        stream_timeouts=(0.2, 1.0),
    )

    started = time.monotonic()
    with pytest.raises(TransportError):
        list(client.chat_stream(["hi"]))
    assert time.monotonic() - started < 5.0, "the first-token timeout did not fire"


def test_stream_timeouts_leave_a_healthy_stream_alone(gateway: MockGateway) -> None:
    gateway.route("/v1/stream", _sse_handler)
    client = Client(
        "openrouter",
        "test-model",
        api_key="sk-test",
        base_url=gateway.base_url + "/stream",
        stream_timeouts=(5.0, 5.0),
        default_headers={"X-Tenant": "acme"},
    )

    chunks = list(client.chat_stream(["hi"]))

    assert [c.text for c in chunks if c.text is not None] == ["Hel", "lo"]
    assert gateway.headers_log[-1]["x-tenant"] == "acme"


def test_warnings_reports_an_unknown_model() -> None:
    # No gateway: warnings() must answer from the catalog alone.
    client = Client("deepseek", "totally-unknown-model", api_key="sk-test")

    warnings = client.warnings(["hi"])

    assert len(warnings) == 1
    assert warnings[0].kind == "unknown_model"
    assert warnings[0].provider == "deepseek"
    assert warnings[0].model == "totally-unknown-model"


def test_warnings_reports_a_deprecated_model() -> None:
    client = Client("deepseek", "deepseek-v4-flash", api_key="sk-test")

    warnings = client.warnings(["hi"])

    assert len(warnings) == 1
    assert warnings[0].kind == "deprecated_model"
    assert warnings[0].model == "deepseek-v4-flash"
    assert warnings[0].provider is None


def test_warnings_stay_empty_for_a_known_model() -> None:
    client = Client("deepseek", "deepseek-v4-pro", api_key="sk-test")

    assert client.warnings(["hi"]) == []


def test_refresh_models_returns_the_mock_listing(gateway: MockGateway) -> None:
    def listing(handler: MockHandler) -> None:
        handler._respond(200, {"data": [{"id": "mock-b"}, {"id": "mock-a"}]})

    gateway.route("/v1/models", listing)
    client = make_client(gateway)

    assert set(client.refresh_models()) == {"mock-a", "mock-b"}


def test_refresh_models_refuses_a_provider_without_a_listing(
    gateway: MockGateway,
) -> None:
    client = Client(
        "zhipu", "test-model", api_key="sk-test", base_url=gateway.base_url
    )

    with pytest.raises(ConfigError):
        client.refresh_models()


def test_async_refresh_models_returns_the_listing(gateway: MockGateway) -> None:
    def listing(handler: MockHandler) -> None:
        handler._respond(200, {"data": [{"id": "mock-b"}, {"id": "mock-a"}]})

    gateway.route("/v1/models", listing)
    client = AsyncClient(
        "openrouter",
        "test-model",
        api_key="sk-test",
        base_url=gateway.base_url,
    )

    async def main() -> list[str]:
        return await client.refresh_models()

    assert set(asyncio.run(main())) == {"mock-a", "mock-b"}
