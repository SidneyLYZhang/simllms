# Structured output is native-only; we never simulate it with prompts

`chat()` accepts a caller-declared JSON schema only when the model's Capabilities say it honours one. Otherwise the request is **gated**: refused locally, before any network call. We do not fall back to "put the schema in the prompt, parse the reply, retry once on validation failure", and we do not expose that fallback behind an opt-in flag.

The fallback is what the well-known multi-provider libraries do, so a reader will assume its absence is an oversight — which is why this needs recording. The reason we refused it: a fallback silently changes what the caller bought. They ask for a schema guarantee and receive a bet on a prompt, and when the reply fails validation the error points at their schema instead of at the model we were never able to constrain. An opt-in flag does not fix that; it just moves the surprise.

Callers who want prompt-and-validate still can: `Options.extra` passes unmodelled fields through untouched, and their own prompt plus their own validation is honest about which one is load-bearing.
