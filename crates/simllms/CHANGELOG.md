# Changelog

All notable changes to simllms are documented here. The docs site's
changelog page is a generated copy of this file — edit this one only.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- **Both packages now describe themselves truthfully on their registry.**
  `crates/simllms/Cargo.toml` and `crates/simllms-python/pyproject.toml` each
  declare an explicit `include` whitelist, so a packed artifact holds what it
  should and nothing else: for the crate, the source tree plus the English
  README, both license files, the third-party attribution and this changelog;
  for the Python distribution, those five beside the module the build backend
  packs by itself.
- `cargo xtask sync-package` writes the per-package copies of those five files
  (cargo and maturin both refuse to reference anything above the package
  directory), and `cargo xtask check-package` proves they match the root
  originals, that the manifests carry the SPDX license expression, the
  provider keywords and a provider-agnostic one-sentence description, and —
  given `cargo package --list` on stdin — that the packed crate holds exactly
  the whitelist.
- `crates/simllms-python/tests/test_package_inventory.py` opens the built
  `.crate`, wheel and sdist and asserts the same inventory from the artifact
  side: the license files land in the wheel's `*.dist-info/licenses/`, the
  sdist carries what PEP 639 demands, and no `__pycache__` leaks in. CI packs
  all three artifacts and names them to those checks, so a run that skipped
  them cannot report green.
- Links inside the English README now point at the repository rather than at
  repository paths. A registry page renders the copy that sits inside the
  package, where `docs/getting-started.md` and `crates/…/examples/…` are not
  there to be found — and `cargo xtask check-package` now refuses any carried
  file that links to something which does not travel with it.
- **The whole release line runs without publishing anything.**
  `.github/workflows/release.yml` starts on a version tag or on a hand dispatch
  whose `dry_run` input defaults to the rehearsal. The pull-request gate's family
  runs again there; five wheels build from one abi3 extension each (manylinux
  x86_64 and aarch64 from pinned `manylinux_2_28` images, the latter
  cross-compiled rather than emulated, macOS x86_64 and arm64 as two separate
  wheels, Windows amd64); the crate and the sdist come from their own packagers;
  and only `publish-python` and `publish-crate` — each behind the shared
  approval environment and an explicit `dry_run` test — reach a registry. A
  rehearsal skips those two visibly rather than leaving them out of the run.
- `cargo xtask check-release <tag>` refuses a tag with a pre-release or build
  suffix, a tag whose number differs from the Authoritative Version, a tag on a
  commit that has not landed on `main`, and a version the changelog holds no
  dated section for — naming the rule that fired each time. `cargo xtask
  release-tag` answers what tag this commit would become, which is the only
  question a rehearsal can ask.
- `cargo xtask check-workflow` reads the release file and holds it to itself: the
  triggers, the pinned base images, the absence of an emulator, one abi3 wheel
  per platform, build and publish as separate jobs joined by artifacts, and every
  gate the pull-request path runs. It also requires the release file to *run* the
  verdicts that decide a release — `check-release` and `check-artifacts` — and to
  hand the seam its tag and its wheel, because a deleted step leaves a file that
  is still well-formed and a rule nobody invokes stops nothing.
  `cargo xtask check-artifacts <dir> --tag <tag>` compares a built artifact set
  against the intended platforms, so a missing platform and an unexpected extra
  both go red.
- `crates/simllms-python/tests/test_release_artifacts.py` runs against an
  installed wheel in a venv that never saw the checkout: the reported version is
  the tag's, the version comes out of the compiled extension, the licenses and
  the attribution sit where an installer looks, a Gate refusal raises the typed
  capability error without a network call, and the request debugger renders an
  outbound body with no credential in it. The release path fails the run if any
  of those checks skipped.

### Changed

- The crate's whitelist names `src/*.rs` and `examples/*.rs` rather than the two
  directories. `cargo package` reaches into a listed directory without asking
  version control, so a bare `src` would publish an ignored neighbour of the
  sources — a stray `src/__pycache__/` or an editor's backup. `check-package`
  refuses a directory entry, and the release job plants a marker inside every
  ignored directory before packing, so the artifact checks prove it stayed out.
- The Python distribution declares `license = "MIT OR Apache-2.0"` as an SPDX
  expression with `license-files` (PEP 639, `Metadata-Version: 2.4`) instead
  of a `License ::` classifier, which PyPI deprecated. The wheels are built
  with `maturin>=1.9.3`.
- crates.io accepts five keywords, so the crate carries `llm`,
  `openai-compatible` and three provider names; PyPI has no such cap and
  carries all twelve. `check-package` requires the crate's set to be a subset
  of the Python one.

## [0.1.0] - 2026-10-16

First working release: one request shape, one response shape, and a
per-model statement of what each model can actually do — checked locally,
before any network call.

### Added

- **Unified chat client** for DeepSeek, Moonshot (Kimi), Qwen/DashScope,
  Zhipu (GLM), Xiaomi MiMo, OpenRouter, and any OpenAI-compatible gateway
  (`Provider::Custom`), as both a Rust crate and a Python package.
- **Capability Gate**: impossible requests are refused locally with the
  missing capability named (and, when the catalog knows any, capable models
  suggested). Unknown model ids warn and pass (ADR-0005).
- **Capability catalog** vendored from a filtered models.dev snapshot
  (515 entries), regenerated by `cargo xtask refresh-catalog`; hand-
  maintained image-generation overrides where providers publish nothing.
- **`refresh_models()`** (Rust only in this release) consults the provider's own model listing
  (DeepSeek, Moonshot, DashScope, OpenRouter) and merges it under the
  precedence user > provider > catalog.
- **Streaming** (`chat_stream`) normalized into text/reasoning deltas plus
  a final chunk with finish reason and usage; first-token and idle
  timeouts; drop cancels the request.
- **Structured output**, native-only (ADR-0004): the schema reaches the
  provider as a constrained `response_format`, never as prompt text;
  refusals surface locally with alternatives.
- **Image generation** across three provider wire shapes (Zhipu sync URL,
  OpenRouter sync b64, DashScope submit-then-poll), normalized to a
  bytes-or-URL result; unsupported parameters are refused with the
  parameter and model named.
- **Retries** with exponential backoff and Retry-After support (429/5xx
  and transport errors only), disabled per client when wanted.
- **Error layers** (`Capability` / `Config` / `Api` / `Transport`) with the
  provider's `raw_body` preserved verbatim as evidence.
- **Python bindings** (pyo3, abi3-py310): `Client` with the same gating and
  error mapping, `AsyncClient` as an executor-bound thread bridge, sync and
  async streaming, `generate_image`, `debug_request`, and standard logging
  under the fixed `"simllms"` logger.
- **CI**: hermetic PR gate (Rust + Python) with an offline generated-catalog
  check, and a weekly workflow that opens a reviewable catalog-bump PR —
  never auto-merged (ADR-0002).

[0.1.0]: https://github.com/SidneyLYZhang/simllms/releases/tag/v0.1.0
