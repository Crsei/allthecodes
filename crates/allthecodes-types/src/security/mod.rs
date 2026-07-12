//! Shared security contracts used across runtime, permission, and audit crates.

pub mod taint;

pub use taint::{
    clear_recent_security_denials, recent_security_denials, record_security_denial,
    SecurityDenialSummary, TaintContext, TaintDecision, TaintDecisionKind, TaintMark, TaintSink,
    TrustLevel, UntrustedSourceKind,
};
