-- Extend the closed workspace consumer catalog without changing existing routes.
alter table network_egress_routes
    drop constraint network_egress_routes_consumer_kind_check,
    drop constraint network_egress_routes_check;

alter table network_egress_routes
    add constraint network_egress_routes_consumer_kind_check
        check (consumer_kind in ('github', 'model_provider', 'http_node', 'agent_logs_exchange')),
    add constraint network_egress_routes_check check (
        (consumer_kind in ('github', 'http_node', 'agent_logs_exchange') and consumer_reference is null)
        or consumer_kind = 'model_provider'
    );
