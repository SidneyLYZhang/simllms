"""Chat example: talks to the live DeepSeek API when DEEPSEEK_API_KEY is set,
or to a local mock server otherwise.

Run with: uv run examples/chat.py  (from crates/simllms-python)
"""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))

from _mock import has_key, mock_gateway  # noqa: E402

from simllms import Client  # noqa: E402


def main() -> None:
    if has_key("DEEPSEEK_API_KEY"):
        client = Client("deepseek", "deepseek-v4-pro", os.environ["DEEPSEEK_API_KEY"])
        reply = client.chat(["用一句话介绍 deepseek 是什么。"])
        print(reply.content)
        if reply.total_tokens is not None:
            print(f"[usage] total tokens: {reply.total_tokens}")
        return

    with mock_gateway() as gw:
        client = Client("deepseek", "deepseek-v4-pro", "mock-key", base_url=gw.base_url)
        reply = client.chat(["你好"])
        print(f"{reply.content} (本地模拟服务 — 设置 DEEPSEEK_API_KEY 后走真实 API)")
        print(f"[usage] total tokens: {reply.total_tokens}")


if __name__ == "__main__":
    main()
