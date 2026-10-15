use super::NetworkEgressConsumerSelector;
use uuid::Uuid;

#[test]
fn agent_logs_exchange_has_its_own_closed_workspace_selector() {
    let selector = NetworkEgressConsumerSelector::AgentLogsExchange;
    assert_eq!(selector.consumer_kind(), "agent_logs_exchange");
    assert_eq!(selector.consumer_reference(), None);
    assert_eq!(
        NetworkEgressConsumerSelector::from_storage("agent_logs_exchange", None).unwrap(),
        selector
    );
    assert!(
        NetworkEgressConsumerSelector::from_storage("agent_logs_exchange", Some(Uuid::now_v7()))
            .is_err()
    );
    assert!(NetworkEgressConsumerSelector::from_storage("langfuse", None).is_err());
}
