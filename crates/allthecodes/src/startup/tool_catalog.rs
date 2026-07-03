use crate::startup::startup_context::StartupContext;

pub(crate) struct ToolCatalogBuilder;

impl ToolCatalogBuilder {
    pub(crate) async fn build(_startup: &StartupContext) -> anyhow::Result<()> {
        Ok(())
    }
}
