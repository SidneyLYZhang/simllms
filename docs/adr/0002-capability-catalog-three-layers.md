# Capabilities come from a vendored models.dev snapshot, with two override layers

To gate requests locally we need a per-model statement of what that model can do. We decided on three layers, highest wins: `user` (declared on the client) > `provider` (fetched on demand, only for the four providers that publish machine-readable capability endpoints) > `catalog` (a dated snapshot of [models.dev](https://models.dev) data, MIT, vendored into `data/` and **codegen'd into Rust constants at build time**). Runtime HTTP is never on the request path.

models.dev won over hand-maintaining six providers because it already carries exactly the fields gating needs (`reasoning_options`, `tool_call`, `structured_output`, `modalities.input/output`, `limit.context/output`, `status: deprecated`) across 230 providers with CI-validated schemas. Codegen rather than shipping a 5.6 MB JSON keeps the offline path deterministic and keeps `Capabilities` reachable without a parser at call time.

## Consequences

- Snapshot updates arrive as a weekly PR that a human merges, never an auto-merge: a silently changed catalog would make user behaviour differ between two runs of the same code.
- Only DeepSeek, Moonshot, DashScope and OpenRouter can be refreshed at runtime. Zhipu and MiMo have model-listing endpoints that answer 401 but publish no documented schema, so they are catalog + user-override only.
- No `/v1/models` endpoint on any provider covers image generation, and models.dev is chat-only for these six. Image-generation capabilities are therefore hand-maintained overrides regardless of what the catalog says.
- The catalog carries `pricing` and we surface it as metadata, but the library never multiplies it into a cost for the caller. Prices vary by region, by tier (`_above_128k`), and by currency, and our snapshot is a week stale at worst; turning that into a money figure would make a stale catalog our user's billing error. `Usage` reports token counts only.
