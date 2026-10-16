# simllms

A single client for many LLM providers: one request shape, one response shape, and a per-model statement of what that model is actually able to do. Exists so callers describe intent once and stop re-learning six vendor dialects.

## Language

### Channels and models

**Provider**:
The service that answers requests and issues the credential — DeepSeek, Moonshot, DashScope, Zhipu, MiMo, OpenRouter, or a user-supplied one. A provider is a channel, not a model.
_Avoid_: Vendor, backend, upstream

**Dialect**:
The wire schema a provider speaks for the same intent. A provider may speak several at once.
_Avoid_: API style, format, mode

**Model Id**:
The name a *specific provider* uses for a model. Always provider-local; the same underlying model has a different Model Id on each provider.
_Avoid_: Model name, slug

**Canonical Model**:
The provider-independent identity that several Model Ids across providers stand for. A catalog notion used for cross-references and suggestions, never something a caller passes.
_Avoid_: Global model id, base model

### What a model can do

**Capability**:
One asserted behaviour or input modality of a model — accepts image input, reasons on demand, honours a JSON schema. The unit of gating.
_Avoid_: Feature, flag, support

**Gate**:
The act of refusing a request locally, before any network call, because the model's Capabilities do not cover it.
_Avoid_: Validate, check

**Catalog**:
The built-in table of Models and their Capabilities, shipped with the package.
_Avoid_: Registry, database, index

**Capability Source**:
Where a Capability came from: `catalog` (shipped), `provider` (fetched from that provider's own model listing), or `user` (declared on the client). Higher sources override lower ones.
_Avoid_: Authority, origin

**Snapshot**:
The dated copy of upstream Catalog data vendored into the repo, so behaviour is reproducible between releases.
_Avoid_: Latest, cache

### Talking to a model

**Client**:
A provider + model + credential + base URL + Options bound together. The object you create once and call repeatedly.
_Avoid_: Session, connection, instance

**Options**:
The knobs applied to every request from a Client — temperature, output ceiling, thinking level, and anything not modelled, passed through untouched.
_Avoid_: model_options, config, params

**Content Block**:
One piece of a Message's payload: text or an image. The unit that Capabilities are asserted against.
_Avoid_: Part, chunk, segment

**Message**:
A Role plus an ordered list of Content Blocks. History is a list of Messages supplied by the caller; the library keeps no conversation state.
_Avoid_: Turn, prompt

**Chat**:
Sending Messages and receiving an assistant Message, either whole or in a stream.
_Avoid_: Completion, query

**Structured Output**:
An assistant answer constrained to a caller-declared schema. Available only where the model's Capabilities say so; never simulated by prompt rewriting.
_Avoid_: JSON mode, function result

**Usage**:
Token counts reported for a request — input, output, reasoning, cached. Counts only; the library does not compute cost.
_Avoid_: Stats, metrics, billing

**Image Generation**:
Producing an image from a prompt. Distinct from Chat: different providers, different endpoints, some of which complete asynchronously behind the scenes.
_Avoid_: Plot, drawing, text2image

### Releasing

**Release**:
One act that freezes current code plus one Catalog Snapshot into the same version number on both registries. Rust and Python never release apart.
_Avoid_: Publish, deploy, ship

**Authoritative Version**:
The single place that decides a Release's number — the workspace version in the root `Cargo.toml`. Every other version in the tree is a derived copy.
_Avoid_: Master version, canonical version

**Registry**:
The public index a Release is made available on: crates.io for the crate, PyPI for the Python distribution. It is outside the package, unlike a Catalog Snapshot, which travels inside it.
_Avoid_: Catalog, marketplace

**Forward Fix**:
Repairing a half-failed Release by cutting the next number, not by removing what already shipped. Registries keep old numbers forever; we do not undo them.
_Avoid_: Rollback, yank
