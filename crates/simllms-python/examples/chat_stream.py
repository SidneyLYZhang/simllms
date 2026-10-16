"""Streaming example: talks to the live Moonshot API when MOONSHOT_API_KEY is
set, or to a local mock SSE server otherwise.

Run with: uv run examples/chat_stream.py  (from crates/simllms-python)
"""

import asyncio
import os
import sys

sys.path.insert(0, os.path.dirname(__file__))

from _mock import has_key, mock_gateway  # noqa: E402

from simllms import AsyncClient  # noqa: E402


async def stream_demo(client: AsyncClient, prompt: str) -> None:
    async for chunk in client.chat_stream([prompt]):
        if chunk.text is not None:
            print(chunk.text, end="", flush=True)
        if chunk.finish_reason is not None:
            print(f"\n[finish] {chunk.finish_reason}")
        if chunk.total_tokens is not None:
            print(f"[usage] total tokens: {chunk.total_tokens}")


async def main() -> None:
    if has_key("MOONSHOT_API_KEY"):
        client = AsyncClient("moonshot", "kimi-k2", os.environ["MOONSHOT_API_KEY"])
        await stream_demo(client, "用三句话介绍月之暗面。")
        return

    with mock_gateway() as gw:
        client = AsyncClient("moonshot", "kimi-k2", "mock-key", base_url=gw.base_url)
        print("(本地模拟服务 — 设置 MOONSHOT_API_KEY 后走真实 API)")
        await stream_demo(client, "你好")


if __name__ == "__main__":
    asyncio.run(main())
