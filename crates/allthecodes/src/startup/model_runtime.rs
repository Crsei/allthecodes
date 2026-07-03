use crate::startup::startup_context::StartupContext;

pub(crate) struct ModelRuntimeBuilder;

impl ModelRuntimeBuilder {
    pub(crate) async fn build(_startup: &StartupContext) -> anyhow::Result<()> {
        Ok(())
    }
}
