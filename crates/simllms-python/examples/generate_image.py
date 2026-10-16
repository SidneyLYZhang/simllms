"""Image generation example: the same call normalizes three provider wire
shapes (Zhipu sync URL, OpenRouter sync b64, DashScope submit-then-poll).
Talks to live Zhipu when ZHIPU_API_KEY is set, or to a local mock otherwise.

Run with: uv run examples/generate_image.py  (from crates/simllms-python)
"""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))

from _mock import has_key, mock_gateway  # noqa: E402

from simllms import Client, GeneratedImage  # noqa: E402


def show(images: list[GeneratedImage]) -> None:
    for image in images:
        # bytes-or-URL invariant: exactly one of data / url is set.
        if image.url is not None:
            print(f"image url: {image.url}")
        elif image.data is not None:
            print(f"image bytes: {len(image.data)} received")
        if image.revised_prompt is not None:
            print(f"revised prompt: {image.revised_prompt}")


def main() -> None:
    if has_key("ZHIPU_API_KEY"):
        client = Client("zhipu", "cogview-4", os.environ["ZHIPU_API_KEY"])
        show(client.generate_image("水彩风格的中国龙", size="1024x1024"))
        return

    with mock_gateway() as gw:
        client = Client("zhipu", "cogview-4", "mock-key", base_url=gw.base_url)
        print("(本地模拟服务 — 设置 ZHIPU_API_KEY 后走真实 API)")
        show(client.generate_image("水彩风格的中国龙"))


if __name__ == "__main__":
    main()
