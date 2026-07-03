use std::sync::Arc;

use crate::types::tool::Tools;

pub trait ToolRegistryService: Send + Sync {
    fn active_tools(&self) -> Tools;
}

pub trait PermissionMessageResolver: Send + Sync {
    fn resolve_permission_message(&self, tool_name: &str) -> Option<String>;
}

pub trait HookRunnerService: Send + Sync {
    fn hook_runner(&self) -> Arc<dyn allthecodes_types::hooks::HookRunner>;
}

pub trait CommandDispatcherService: Send + Sync {
    fn command_dispatcher(&self) -> Arc<dyn allthecodes_types::commands::CommandDispatcher>;
}

pub trait ModelClientFactoryService: Send + Sync {
    fn client_for_backend(
        &self,
        backend_name: Option<&str>,
    ) -> Option<Arc<allthecodes_api::api::client::ApiClient>>;
}

#[derive(Clone)]
pub struct RuntimeServices {
    pub tool_registry: Arc<dyn ToolRegistryService>,
    pub permission_message_resolver: Arc<dyn PermissionMessageResolver>,
    pub hook_runner: Arc<dyn HookRunnerService>,
    pub command_dispatcher: Arc<dyn CommandDispatcherService>,
    pub model_client_factory: Arc<dyn ModelClientFactoryService>,
}

impl RuntimeServices {
    pub fn from_process_defaults() -> Self {
        Self {
            tool_registry: Arc::new(ProcessToolRegistryService),
            permission_message_resolver: Arc::new(ProcessPermissionMessageResolver),
            hook_runner: Arc::new(NoopHookRunnerService),
            command_dispatcher: Arc::new(NoopCommandDispatcherService),
            model_client_factory: Arc::new(ProcessModelClientFactoryService),
        }
    }

    pub fn from_static_tools(tools: Tools) -> Self {
        Self {
            tool_registry: Arc::new(StaticToolRegistryService { tools }),
            permission_message_resolver: Arc::new(ProcessPermissionMessageResolver),
            hook_runner: Arc::new(NoopHookRunnerService),
            command_dispatcher: Arc::new(NoopCommandDispatcherService),
            model_client_factory: Arc::new(ProcessModelClientFactoryService),
        }
    }
}

pub struct StaticToolRegistryService {
    tools: Tools,
}

impl ToolRegistryService for StaticToolRegistryService {
    fn active_tools(&self) -> Tools {
        self.tools.clone()
    }
}

struct ProcessToolRegistryService;

impl ToolRegistryService for ProcessToolRegistryService {
    fn active_tools(&self) -> Tools {
        allthecodes_tools::registry::process_default_active_tools()
    }
}

struct ProcessPermissionMessageResolver;

impl PermissionMessageResolver for ProcessPermissionMessageResolver {
    fn resolve_permission_message(&self, tool_name: &str) -> Option<String> {
        allthecodes_permissions::decision::process_descriptive_permission_message(tool_name)
    }
}

struct NoopHookRunnerService;

impl HookRunnerService for NoopHookRunnerService {
    fn hook_runner(&self) -> Arc<dyn allthecodes_types::hooks::HookRunner> {
        Arc::new(allthecodes_types::hooks::NoopHookRunner::new())
    }
}

struct NoopCommandDispatcherService;

impl CommandDispatcherService for NoopCommandDispatcherService {
    fn command_dispatcher(&self) -> Arc<dyn allthecodes_types::commands::CommandDispatcher> {
        Arc::new(allthecodes_types::commands::NoopCommandDispatcher::new())
    }
}

struct ProcessModelClientFactoryService;

impl ModelClientFactoryService for ProcessModelClientFactoryService {
    fn client_for_backend(
        &self,
        backend_name: Option<&str>,
    ) -> Option<Arc<allthecodes_api::api::client::ApiClient>> {
        allthecodes_api::api::client::ApiClient::from_backend(backend_name).map(Arc::new)
    }
}
