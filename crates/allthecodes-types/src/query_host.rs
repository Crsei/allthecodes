/// Host capability needed to run a client-submitted query turn.
///
/// The associated stream type is intentionally unconstrained here so this
/// contract can remain in `cc-types`; transport helpers add their own stream
/// bounds at the call site.
pub trait QueryTurnHost<SdkMessage>: Send + Sync + 'static {
    type Stream: Send + 'static;

    fn reset_abort(&self);
    fn submit_client_message(&self, prompt_text: &str) -> Self::Stream;
}
