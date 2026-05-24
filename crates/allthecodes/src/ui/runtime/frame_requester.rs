// test infrastructure — coalescing frame requester not wired to production loop yet
//! Coalescing frame requester for redraw scheduling.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameReason {
    Input,
    Stream,
    Resize,
    Timer,
    Overlay,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FrameRequester {
    pending: bool,
    request_count: usize,
    last_reason: Option<FrameReason>,
}

impl FrameRequester {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request(&mut self, reason: FrameReason) {
        self.pending = true;
        self.request_count += 1;
        self.last_reason = Some(reason);
    }

    pub fn is_pending(&self) -> bool {
        self.pending
    }

    pub fn take_pending(&mut self) -> bool {
        let pending = self.pending;
        self.pending = false;
        pending
    }

    pub fn snapshot(&self) -> FrameRequestSnapshot {
        FrameRequestSnapshot {
            pending: self.pending,
            request_count: self.request_count,
            last_reason: self.last_reason,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameRequestSnapshot {
    pub pending: bool,
    pub request_count: usize,
    pub last_reason: Option<FrameReason>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalesces_pending_requests_and_tracks_last_reason() {
        let mut requester = FrameRequester::new();
        assert!(!requester.is_pending());

        for reason in [
            FrameReason::Input,
            FrameReason::Stream,
            FrameReason::Resize,
            FrameReason::Timer,
            FrameReason::Overlay,
        ] {
            requester.request(reason);
        }

        assert!(requester.is_pending());
        let snapshot = requester.snapshot();
        assert_eq!(snapshot.request_count, 5);
        assert_eq!(snapshot.last_reason, Some(FrameReason::Overlay));
        assert!(requester.take_pending());
        assert!(!requester.is_pending());
    }
}
