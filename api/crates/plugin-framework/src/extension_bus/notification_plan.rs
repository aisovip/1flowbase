//! Host-owned managed notification contribution point. Execution uses the existing dispatcher.
use super::*;

pub fn notification_extension_point(
    owner_module_id: ModuleId,
) -> Result<ExtensionPointDescriptor, DescriptorValueError> {
    Ok(ExtensionPointDescriptor {
        point_id: ExtensionPointId::new(extension_contracts::NOTIFICATION_POINT)?,
        owner_module_id,
        point_kind: ExtensionPointKind::ResourceAction,
        contract: ContractDescriptor::new(
            "notification",
            extension_contracts::NOTIFICATION_CONTRACT_V1,
        )?,
        scope: ScopeSemantics::Workspace,
        cardinality: Cardinality::Many,
        ordering: OrderingSemantics::Lexicographic,
        failure: FailureSemantics::FailClosed,
        delivery: DeliverySemantics::Synchronous,
        lifecycle: LifecycleSemantics::Invocation,
        allowed_permissions: [PermissionCode::new(
            extension_contracts::NOTIFICATION_SEND_PERMISSION,
        )?]
        .into_iter()
        .collect(),
        override_policy: OverridePolicy::Sealed,
    })
}
