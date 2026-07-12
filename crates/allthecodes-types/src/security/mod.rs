//! Shared security contracts used across runtime, permission, and audit crates.

pub mod taint;

pub use taint::{
    TaintContext, TaintDecision, TaintDecisionKind, TaintMark, TaintSink, TrustLevel,
    UntrustedSourceKind,
};
