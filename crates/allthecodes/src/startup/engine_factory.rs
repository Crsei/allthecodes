use crate::startup::startup_context::StartupContext;

pub(crate) struct EngineFactory;

impl EngineFactory {
    pub(crate) async fn build(_startup: &StartupContext) -> anyhow::Result<()> {
        Ok(())
    }
}
