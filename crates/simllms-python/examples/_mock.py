"""Shared local mock gateway for the simllms examples.

Every example talks to the live provider when its API key is present, and
to this zero-credential mock otherwise — so every example runs out of the
box. It speaks the OpenAI-compatible wire shape the Rust core emits.
"""

from __future__ import annotations

import json
import os
import threading
from collections.abc import Iterator
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from contextlib import contextmanager
from typing import Any

SUCCESS_BODY: dict[str, Any] = {
    "choices": [{"message": {"role": "assistant", "content": "(mock reply) 你好，这是本地模拟服务。"}}],
    "usage": {"prompt_tokens": 9, "completion_tokens": 12, "total_tokens": 21},
}

SSE_BODY: str = (
    'data: {"choices":[{"index":0,"delta":{"role":"assistant","content":"模拟"}}]}\n\n'
    'data: {"choices":[{"index":0,"delta":{"content":"流式输出"}}]}\n\n'
    'data: {"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],'
    '"usage":{"prompt_tokens":8,"completion_tokens":6,"total_tokens":14}}\n\n'
    "data: [DONE]\n\n"
)


class _Handler(BaseHTTPRequestHandler):
    def log_message(self, format: str, *args: Any) -> None:  # noqa: A002
        pass

    def do_POST(self) -> None:
        length = int(self.headers.get("Content-Length", "0"))
        raw = self.rfile.read(length)
        body = json.loads(raw) if raw else {}
        if self.path.endswith("/chat/completions") and body.get("stream") is True:
            # Streaming request: answer with an SSE body.
            self._sse()
            return
        if self.path.endswith("/chat/completions"):
            if body.get("response_format") is not None:
                structured = dict(SUCCESS_BODY)
                structured["choices"] = [
                    {"message": {"role": "assistant", "content": '{"capital": "Paris", "population_millions": 2.1}'}}
                ]
                self._json(structured)
            else:
                self._json(SUCCESS_BODY)
            return
        if self.path.endswith("/images/generations"):
            self._json({"data": [{"url": "https://img.example/dragon.png", "revised_prompt": "水彩中国龙"}]})
            return
        if self.path.endswith("/images"):
            import base64

            self._json({"data": [{"b64_json": base64.b64encode(b"PNG-bytes").decode("ascii")}]})
            return
        if self.path.endswith("/image-synthesis"):
            self._json({"output": {"task_id": "t-1", "task_status": "PENDING"}})
            return
        self._json(SUCCESS_BODY)

    def do_GET(self) -> None:
        if "/api/v1/tasks/" in self.path:
            self._json(
                {
                    "output": {
                        "task_status": "SUCCEEDED",
                        "results": [{"url": "https://img.example/q.png"}],
                    }
                }
            )
            return
        self._json({"data": [{"id": "mock-model"}]})

    def _json(self, body: dict[str, Any]) -> None:
        payload = json.dumps(body).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def _sse(self) -> None:
        payload = SSE_BODY.encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


class MockGateway:
    """Serves on 127.0.0.1:<random port>."""

    def __init__(self) -> None:
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), _Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    @property
    def base_url(self) -> str:
        host, port = self.server.server_address[:2]
        return f"http://{host}:{port}/v1"


@contextmanager
def mock_gateway() -> Iterator[MockGateway]:
    gw = MockGateway()
    gw.thread.start()
    try:
        yield gw
    finally:
        gw.server.shutdown()
        gw.server.server_close()


def has_key(env_var: str) -> bool:
    """True when the env var is set to a non-empty value."""
    return bool(os.environ.get(env_var))
