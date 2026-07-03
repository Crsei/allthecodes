use crate::startup::startup_context::StartupContext;

pub(crate) struct AppStateFactory;

impl AppStateFactory {
    pub(crate) async fn build(_startup: &StartupContext) -> anyhow::Result<()> {
        Ok(())
    }
}
