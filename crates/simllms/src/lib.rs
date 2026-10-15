//! Unified client for multiple LLM providers.
//!
//! One request shape, one response shape, and a per-model statement of what
//! that model is actually able to do. See `CONTEXT.md` for the domain
//! vocabulary and `docs/adr/` for the decisions behind the shape.

pub mod capabilities;
pub mod catalog;
pub mod catalog_data;
pub mod client;
pub mod dialect;
pub mod error;
pub mod gate;
pub mod message;
pub mod options;
pub mod provider;
pub mod reply;
pub mod stream;

pub use capabilities::{Capabilities, CapabilityOverrides, Modality, ModelStatus, ReasoningMode, ResolvedCapabilities, Source};
pub use catalog::Catalog;
pub use client::{Auth, Client, RetryPolicy};
pub use error::{ApiError, CapabilityError, ConfigError, Error, MissingCapability, TransportError};
pub use gate::{gate, ChatRequest, GateOutcome, Warning};
pub use message::{ContentBlock, ImageSource, Message, Role};
pub use options::{Options, ReasoningEffort};
pub use provider::Provider;
pub use reply::{Reply, Usage};
pub use stream::{ChatStream, Chunk, Delta};
