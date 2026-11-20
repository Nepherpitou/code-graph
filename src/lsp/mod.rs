//! LSP feature: JSON-RPC transport, protocol types, retries, and the client.
//! `client` is the only module that talks to the server; `transport` owns
//! framing and request dispatch.

mod client;
mod progress;
mod protocol;
mod transport;

pub use client::{start, LspClient};
pub(crate) use progress::{wait_for_progress, Waiting};
pub use protocol::{CallHierarchyItem, LspPosition, LspRange, OutgoingCall};
