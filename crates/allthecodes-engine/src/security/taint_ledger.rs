use std::collections::BTreeMap;

use allthecodes_types::security::{TaintContext, TaintMark, UntrustedSourceKind};

const MAX_MARKS_PER_TURN: usize = 128;
const MAX_TOOL_RESULTS_PER_TURN: usize = 128;
const OVERFLOW_SOURCE_ID: &str = "taint-ledger:overflow";

/// Turn-scoped provenance accumulated from untrusted tool and ingress content.
#[derive(Debug, Clone, Default)]
pub struct TaintLedger {
    active_turn: TaintContext,
    by_tool_use_id: BTreeMap<String, TaintContext>,
}

impl TaintLedger {
    pub fn register_tool_result(&mut self, tool_use_id: &str, taint: TaintContext) {
        if taint.marks.is_empty() {
            return;
        }

        if self.by_tool_use_id.len() < MAX_TOOL_RESULTS_PER_TURN
            || self.by_tool_use_id.contains_key(tool_use_id)
        {
            self.by_tool_use_id
                .insert(tool_use_id.to_owned(), taint.clone());
        }

        let marks = self
            .active_turn
            .marks
            .iter()
            .cloned()
            .chain(taint.marks)
            .collect::<Vec<_>>();
        self.active_turn = bounded_context(marks);
    }

    pub fn active_context(&self) -> TaintContext {
        self.active_turn.clone()
    }

    pub fn context_for_tool_use(&self, tool_use_id: &str) -> Option<&TaintContext> {
        self.by_tool_use_id.get(tool_use_id)
    }

    pub fn clear_at_trusted_turn_boundary(&mut self) {
        self.active_turn = TaintContext::default();
        self.by_tool_use_id.clear();
    }
}

fn bounded_context(marks: Vec<TaintMark>) -> TaintContext {
    let mut context = TaintContext::from_marks(marks);
    if context.marks.len() <= MAX_MARKS_PER_TURN {
        return context;
    }

    let overflow = context.marks.split_off(MAX_MARKS_PER_TURN - 1);
    let mut digest_material = String::new();
    for mark in overflow {
        digest_material.push_str(&format!(
            "{:?}\0{}\0{}\n",
            mark.source, mark.source_id, mark.digest
        ));
    }
    context.marks.push(TaintMark::from_content(
        UntrustedSourceKind::ToolOutput,
        OVERFLOW_SOURCE_ID,
        digest_material.as_bytes(),
    ));
    TaintContext::from_marks(context.marks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(id: usize) -> TaintMark {
        TaintMark::from_content(
            UntrustedSourceKind::WebContent,
            format!("web:{id:03}"),
            format!("content:{id}").as_bytes(),
        )
    }

    #[test]
    fn registers_deduplicates_and_clears_turn_marks() {
        let mut ledger = TaintLedger::default();
        let taint = TaintContext::from_marks([mark(1), mark(1)]);
        ledger.register_tool_result("tool-1", taint);

        assert_eq!(ledger.active_context().marks.len(), 1);
        assert!(ledger.context_for_tool_use("tool-1").is_some());

        ledger.clear_at_trusted_turn_boundary();
        assert!(!ledger.active_context().is_untrusted());
        assert!(ledger.context_for_tool_use("tool-1").is_none());
    }

    #[test]
    fn overflow_retains_a_deterministic_untrusted_aggregate() {
        let mut ledger = TaintLedger::default();
        let marks = (0..140).map(mark).collect::<Vec<_>>();
        ledger.register_tool_result("tool-overflow", TaintContext::from_marks(marks));

        let active = ledger.active_context();
        assert_eq!(active.marks.len(), MAX_MARKS_PER_TURN);
        assert!(active.is_untrusted());
        assert!(active
            .marks
            .iter()
            .any(|mark| mark.source_id == OVERFLOW_SOURCE_ID));
    }
}
