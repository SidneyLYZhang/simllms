# The pure-Rust core never links Python

We ship one cargo crate and one Python package from this repo. We decided the Python bindings live in a *separate* crate that depends on the pure-Rust core, and that the bindings crate is **not a member of the top-level cargo workspace** — it carries its own empty `[workspace]` table and its own `Cargo.lock`, following the layout lance uses for `python/`.

The core reason is the failure mode this choice prevents: if a `cargo build` at the repo root reached a PyO3 crate, every Rust user — including the ones who will never touch Python — would need Python headers on their machine to build. Keeping the bindings out of the root workspace makes `cargo build` at the root provably Python-free. The cost we accepted is two `Cargo.lock` files whose dependency versions must be aligned by hand at release time.

## Considered Options

- Single crate with a `python` feature gate: rejected — the feature is a speed bump, not a boundary; a stray `pyo3` dependency anywhere in the core would leak through it.
- Bindings as a workspace member: rejected — root `cargo build` then requires a Python toolchain.
- Two fully independent implementations, one per language: rejected before this — provider behaviour would drift between them with nothing to reconcile against.
