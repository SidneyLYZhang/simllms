"""The Catalog as a queryable surface, and overrides as a typed object.

Spec #23 / ticket #25: `capabilities()` and `models_with()` answer from the
vendored catalog alone (no network, no mock server needed); the wire-level
override tests prove `CapabilityOverrides` still steers the Gate end to end.
"""

from __future__ import annotations

import pytest

from test_client import MockGateway, gateway  # noqa: F401 — re-exported fixture

import simllms
from simllms import (
    Capabilities,
    CapabilityError,
    CapabilityOverrides,
    Client,
    Source,
    capabilities,
    models_with,
)

IMAGE_MESSAGES = [
    {
        "role": "user",
        "content": [
            {"type": "text", "text": "what is this?"},
            {"type": "image_url", "url": "https://example.com/cat.png"},
        ],
    }
]

# The gate refuses locally; base_url points at a dead port so a wrongly
# un-refused request would surface as TransportError, not a silent pass.
DEAD_END = "http://127.0.0.1:1"


def test_a_known_model_resolves_with_typed_shapes() -> None:
    caps = capabilities("deepseek", "deepseek-v4-pro")

    assert caps is not None
    assert isinstance(caps, Capabilities)
    assert caps.source is Source.CATALOG
    assert "text" in caps.input_modalities
    assert caps.structured_output is True
    assert caps.context_window is not None
    assert caps.status == "active"


def test_an_unknown_model_resolves_to_none() -> None:
    # ADR-0005: the catalog miss is a query answer (None), not an exception.
    assert capabilities("deepseek", "totally-unknown-model") is None


def test_an_unknown_provider_raises() -> None:
    with pytest.raises(TypeError, match="nosuch"):
        capabilities("nosuch", "any-model")
    with pytest.raises(TypeError, match="nosuch"):
        models_with("nosuch")


def test_every_filter_kwarg_returns_sorted_ids() -> None:
    for filters in (
        {"tool_call": True},
        {"structured_output": True},
        {"image_generation": True},
        {"supports_image_input": True},
        {"reasoning": True},
    ):
        ids = models_with("deepseek", **filters)
        assert ids == sorted(ids), filters


def test_filters_are_self_consistent_with_capabilities() -> None:
    for model in models_with("deepseek", tool_call=True):
        caps = capabilities("deepseek", model)
        assert caps is not None
        assert caps.tool_call is True
    for model in models_with("deepseek", structured_output=True):
        caps = capabilities("deepseek", model)
        assert caps is not None
        assert caps.structured_output is True
    for model in models_with("deepseek", supports_image_input=True):
        caps = capabilities("deepseek", model)
        assert caps is not None
        assert "image" in caps.input_modalities
    for model in models_with("deepseek", reasoning=True):
        caps = capabilities("deepseek", model)
        assert caps is not None
        assert caps.reasoning != "none"
    for model in models_with("deepseek", image_generation=True):
        caps = capabilities("deepseek", model)
        assert caps is not None
        assert caps.image_generation is True


def test_a_false_filter_excludes_the_true_ones() -> None:
    true_ids = set(models_with("dashscope", tool_call=True))
    false_ids = set(models_with("dashscope", tool_call=False))
    every_id = set(models_with("dashscope"))

    assert true_ids and false_ids
    assert true_ids.isdisjoint(false_ids)
    assert true_ids | false_ids == every_id


def test_image_generation_filter_finds_the_hand_maintained_model() -> None:
    # cogview-4 is a hand-maintained catalog entry, stable across snapshot bumps.
    assert "cogview-4" in models_with("zhipu", image_generation=True)


def test_an_unknown_filter_kwarg_raises() -> None:
    with pytest.raises(TypeError, match="toolcalling"):
        models_with("deepseek", toolcalling=True)


def test_a_filter_value_must_be_a_bool() -> None:
    with pytest.raises(TypeError, match="bool"):
        models_with("deepseek", tool_call="yes")  # type: ignore[arg-type]


def test_an_empty_override_leaves_the_gate_strict() -> None:
    # CapabilityOverrides() with nothing set must override nothing: the
    # text-only model is still refused images.
    client = Client(
        "deepseek",
        "deepseek-v4-pro",
        api_key="sk-test",
        base_url=DEAD_END,
        capability_overrides=CapabilityOverrides(),
    )

    with pytest.raises(CapabilityError) as excinfo:
        client.chat(IMAGE_MESSAGES)

    assert excinfo.value.missing == "ImageInput"


def test_a_typed_override_relaxes_the_gate_on_the_wire(
    gateway: MockGateway,
) -> None:
    client = Client(
        "deepseek",
        "deepseek-v4-pro",
        api_key="sk-test",
        base_url=gateway.base_url,
        capability_overrides=CapabilityOverrides(image_input=True),
    )
    reply = client.chat(IMAGE_MESSAGES)

    assert reply.content == "hello from the mock"


def test_a_dict_override_is_refused_at_the_boundary() -> None:
    with pytest.raises(TypeError, match="CapabilityOverrides"):
        Client(
            "deepseek",
            "deepseek-v4-pro",
            api_key="sk-test",
            base_url=DEAD_END,
            capability_overrides={"image_input": True},  # type: ignore[arg-type]
        )


def test_override_types_are_the_documented_surface() -> None:
    assert simllms.CapabilityOverrides is CapabilityOverrides
    assert simllms.Capabilities is Capabilities
    assert simllms.Source is Source
