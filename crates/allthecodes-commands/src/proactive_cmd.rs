pub struct ProactiveCmdHandler;

#[async_trait::async_trait]
impl crate::CommandHandler for ProactiveCmdHandler {
    async fn execute(
        &self,
        _args: &str,
        _ctx: &mut crate::CommandContext,
    ) -> anyhow::Result<crate::CommandResult> {
        use allthecodes_services::proactive::{global_controller, ProactiveStatus};

        let controller = global_controller();
        let snapshot = controller.snapshot();
        if matches!(
            snapshot.status,
            ProactiveStatus::Active | ProactiveStatus::Paused | ProactiveStatus::ContextBlocked
        ) {
            controller.deactivate("slash_command");
            allthecodes_services::proactive::write_durable_state(false, None)?;
            allthecodes_services::proactive::clear_sleep_state("proactive_disabled")?;
            Ok(crate::CommandResult::Output(
                "Proactive mode disabled.".to_string(),
            ))
        } else {
            controller.activate("slash_command");
            let activated = controller.snapshot();
            allthecodes_services::proactive::write_durable_state(true, activated.next_tick_at)?;
            Ok(crate::CommandResult::Output(
                "Proactive mode enabled. The assistant will continue working from periodic ticks when idle."
                    .to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CommandContext, CommandHandler, CommandResult};
    use allthecodes_bootstrap::SessionId;
    use allthecodes_config::features::{self, FeatureFlags};
    use allthecodes_engine::types::tool::Tool;
    use std::path::Path;
    use std::path::PathBuf;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    struct ProactiveControllerResetGuard(
        &'static allthecodes_services::proactive::ProactiveController,
    );

    impl Drop for ProactiveControllerResetGuard {
        fn drop(&mut self) {
            self.0.deactivate("test-reset");
        }
    }

    struct FeatureOverrideGuard(Option<FeatureFlags>);

    impl FeatureOverrideGuard {
        fn set(flags: FeatureFlags) -> Self {
            let previous = features::runtime_override();
            features::set_runtime_override(flags);
            Self(previous)
        }
    }

    impl Drop for FeatureOverrideGuard {
        fn drop(&mut self) {
            match self.0.take() {
                Some(flags) => features::set_runtime_override(flags),
                None => features::clear_runtime_override(),
            }
        }
    }

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("/tmp/proactive-test"),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn proactive_command_toggles_active_state() {
        let controller = allthecodes_services::proactive::global_controller();
        let _reset_guard = ProactiveControllerResetGuard(controller);
        controller.deactivate("test-reset");

        let handler = ProactiveCmdHandler;
        let mut ctx = test_ctx();
        let first = handler.execute("", &mut ctx).await.unwrap();
        match first {
            CommandResult::Output(text) => assert!(text.contains("Proactive mode enabled")),
            _ => panic!("expected output"),
        }
        assert_eq!(
            controller.snapshot().status,
            allthecodes_services::proactive::ProactiveStatus::Active
        );

        let second = handler.execute("", &mut ctx).await.unwrap();
        match second {
            CommandResult::Output(text) => assert!(text.contains("Proactive mode disabled")),
            _ => panic!("expected output"),
        }
        assert_eq!(
            controller.snapshot().status,
            allthecodes_services::proactive::ProactiveStatus::Inactive
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn proactive_command_publishes_durable_daemon_state() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let controller = allthecodes_services::proactive::global_controller();
        let _reset_guard = ProactiveControllerResetGuard(controller);
        controller.deactivate("test-reset");

        let handler = ProactiveCmdHandler;
        let mut ctx = test_ctx();
        handler.execute("", &mut ctx).await.unwrap();
        let enabled = allthecodes_services::proactive::read_durable_state()
            .unwrap()
            .expect("durable proactive state after enable");
        assert!(enabled.active);
        assert!(enabled.next_tick_at.is_some());

        handler.execute("", &mut ctx).await.unwrap();
        let disabled = allthecodes_services::proactive::read_durable_state()
            .unwrap()
            .expect("durable proactive state after disable");
        assert!(!disabled.active);
        assert!(disabled.next_tick_at.is_none());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn proactive_command_disable_clears_active_sleep_state() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let controller = allthecodes_services::proactive::global_controller();
        let _reset_guard = ProactiveControllerResetGuard(controller);
        controller.activate("test-active");
        allthecodes_services::proactive::write_sleep_state(300, "waiting").unwrap();

        let handler = ProactiveCmdHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("", &mut ctx).await.unwrap();

        match result {
            CommandResult::Output(text) => assert!(text.contains("Proactive mode disabled")),
            _ => panic!("expected output"),
        }
        assert!(allthecodes_services::proactive::active_sleep_state()
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn proactive_command_enables_sleep_tool_without_feature_env() {
        let controller = allthecodes_services::proactive::global_controller();
        let _reset_guard = ProactiveControllerResetGuard(controller);
        let _features = FeatureOverrideGuard::set(FeatureFlags::all_disabled());
        controller.deactivate("test-reset");

        let handler = ProactiveCmdHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("", &mut ctx).await.unwrap();

        match result {
            CommandResult::Output(text) => assert!(text.contains("Proactive mode enabled")),
            _ => panic!("expected output"),
        }
        assert!(allthecodes_tools::exec::SleepTool.is_enabled());
    }
}
