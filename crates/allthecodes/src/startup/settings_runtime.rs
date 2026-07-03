use crate::startup::startup_context::StartupContext;

pub(crate) struct SettingsRuntimeBuilder;

impl SettingsRuntimeBuilder {
    pub(crate) async fn build(_startup: &StartupContext) -> anyhow::Result<()> {
        Ok(())
    }
}
