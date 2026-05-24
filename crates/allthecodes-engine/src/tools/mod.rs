//! Engine-owned native tool implementations.
//!
//! These tools still implement the `Tool` trait owned by this crate. Shared
//! schemas and prompts live in narrower contract crates where possible.

pub mod exec;
