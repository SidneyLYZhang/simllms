# An unknown model id warns and passes; it is not rejected

If a model id is in the catalog, the Gate is strict. If it is not, the request goes through with a structured warning, and the caller can tighten the behaviour by declaring Capabilities on the client. We rejected both "refuse anything not in the catalog" and "unknown models get conservative defaults".

Refusing would be the more principled design and the less usable one: `Provider::Custom` exists precisely so people can point this library at their own vLLM or gateway, whose models appear in no catalog, and provider ids churn under us (DeepSeek retired `deepseek-chat` and `deepseek-reasoner` on 2026-07-24; Xiaomi delists `mimo-v2.5*` on 2026-10-21). Conservative defaults are worse than either option: inferring "unknown ⇒ text-only" silently discards a caller's image input, and capability cannot be inferred from a model family anyway (`glm-5.3` and `glm-4.5v` differ only by a suffix that carries the whole modality answer).

## Consequences

- A typo'd model id produces a warning plus a provider-side error rather than a clean local rejection. We accept that, because the alternative miservices the gateway case.
- Where the provider publishes a machine-readable model listing, we consult it before warning — so the freshness burden falls on the four providers that can answer, not on our release cadence.
