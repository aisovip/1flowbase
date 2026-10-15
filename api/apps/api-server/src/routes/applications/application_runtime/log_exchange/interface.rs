use super::*;

pub(crate) const OPERATIONS: &[&str] = &[
    "applications.logs.otlp.export",
    "applications.logs.otlp.import",
    "applications.logs.langfuse.export",
    "applications.logs.langfuse.import",
];
pub(crate) const DECLARATIONS: &[ConsoleInterfaceDeclaration] = &[
    ConsoleInterfaceDeclaration {
        interface_id: "applications.logs.otlp.export",
        binding_id: "http.console.applications.logs.otlp.export.v1",
        method: "POST",
        path: "/api/console/applications/:id/logs/otlp/export",
        mutating: true,
    },
    ConsoleInterfaceDeclaration {
        interface_id: "applications.logs.otlp.import",
        binding_id: "http.console.applications.logs.otlp.import.v1",
        method: "POST",
        path: "/api/console/applications/:id/logs/otlp/import",
        mutating: true,
    },
    ConsoleInterfaceDeclaration {
        interface_id: "applications.logs.langfuse.export",
        binding_id: "http.console.applications.logs.langfuse.export.v1",
        method: "POST",
        path: "/api/console/applications/:id/logs/langfuse/export",
        mutating: true,
    },
    ConsoleInterfaceDeclaration {
        interface_id: "applications.logs.langfuse.import",
        binding_id: "http.console.applications.logs.langfuse.import.v1",
        method: "POST",
        path: "/api/console/applications/:id/logs/langfuse/import",
        mutating: true,
    },
];
pub(crate) fn compile_registry(
    store: MainDurableStore,
    network: NetworkEgressHttpClientResolver,
) -> Result<
    Arc<interface_runtime::CompiledInterfaceRegistry>,
    interface_runtime::RegistryCompilationError,
> {
    console_interface::compile_registry(
        "api-server.console-application-log-exchange",
        "graph:console-application-log-exchange-v1",
        DECLARATIONS,
        Arc::new(ExchangeAdapter { store, network }),
    )
}
