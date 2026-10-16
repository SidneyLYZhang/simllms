"""Structured output example: native response_format only (ADR-0004 — the
schema is never pasted into the prompt). Talks to live OpenRouter when
OPENROUTER_API_KEY is set, or to a local mock server otherwise.

Run with: uv run examples/structured_output.py  (from crates/simllms-python)
"""

import json
import os
import sys

sys.path.insert(0, os.path.dirname(__file__))

from _mock import has_key, mock_gateway  # noqa: E402

from simllms import Client, SimLLMError  # noqa: E402

SCHEMA = {
    "type": "object",
    "properties": {
        "capital": {"type": "string"},
        "population_millions": {"type": "number"},
    },
    "required": ["capital", "population_millions"],
    "additionalProperties": False,
}


def main() -> None:
    try:
        if has_key("OPENROUTER_API_KEY"):
            client = Client(
                "openrouter", "openai/gpt-4o-mini", os.environ["OPENROUTER_API_KEY"]
            )
            reply = client.chat(
                ["法国的首都是哪座城市？人口约多少？返回 JSON。"],
                options={"response_schema": SCHEMA},
            )
        else:
            with mock_gateway() as gw:
                client = Client(
                    "openrouter", "vendor/model", "mock-key", base_url=gw.base_url
                )
                reply = client.chat(
                    ["法国的首都是哪座城市？"], options={"response_schema": SCHEMA}
                )
        parsed = json.loads(reply.content)
        print(f"capital: {parsed['capital']}, population: {parsed['population_millions']}M")
    except SimLLMError as error:
        # A Gate refusal names the model, the missing capability, and — when
        # the catalog knows any — capable models on the same provider.
        print(f"simllms refused: {error}")


if __name__ == "__main__":
    main()
