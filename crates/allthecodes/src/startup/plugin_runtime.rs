use crate::startup::startup_context::StartupContext;

pub(crate) struct PluginRuntimeBuilder;

impl PluginRuntimeBuilder {
    pub(crate) async fn build(_startup: &StartupContext) -> anyhow::Result<()> {
        Ok(())
    }
}
