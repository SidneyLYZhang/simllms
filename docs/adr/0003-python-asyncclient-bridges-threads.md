# Python's AsyncClient bridges threads; it is not native async

`simllms` Python ships `Client` (sync) and `AsyncClient` (awaitable). `AsyncClient` is **pure Python**: it calls the synchronous native methods through `asyncio.to_thread`. It does not run Rust futures on the asyncio loop.

We picked this because the native path does not exist yet as a supported one. As of pyo3 0.30.1, `#[pyfunction] async fn` is still gated behind the `experimental-async` feature and documented as "still in active development", and the bridge crate `pyo3-async-runtimes` (0.29.0) pins `pyo3 = "0.29"`, so there is no release matching current pyo3. Building the public Python API on an explicitly-unstable feature, or freezing pyo3 a major version behind to get a bridge, both trade a permanent API for a temporary one.

## Consequences

- Concurrency is bounded by the default executor (`min(32, cpu+4)` threads), not by the event loop. Callers doing tens of thousands of simultaneous requests should size the executor themselves or use the Rust API.
- One thread per in-flight request is real cost, and it is deliberate. If native async ever stabilises, `AsyncClient` can be reimplemented underneath without changing a single caller's code — which is exactly why we are willing to ship the worse-looking version now.
