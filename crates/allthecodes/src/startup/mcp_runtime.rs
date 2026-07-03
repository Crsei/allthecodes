use crate::startup::startup_context::StartupContext;

pub(crate) struct McpRuntimeBuilder;

impl McpRuntimeBuilder {
    pub(crate) async fn build(_startup: &StartupContext) -> anyhow::Result<()> {
        Ok(())
    }
}
