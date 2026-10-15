-- Closed selector: a managed plugin must name its host installation.
alter table network_egress_routes
    drop constraint network_egress_routes_consumer_kind_check,
    drop constraint network_egress_routes_check;
alter table network_egress_routes
    add constraint network_egress_routes_consumer_kind_check
        check (consumer_kind in ('github', 'model_provider', 'http_node', 'agent_logs_exchange', 'managed_plugin')),
    add constraint network_egress_routes_check check (
        (consumer_kind in ('github', 'http_node', 'agent_logs_exchange') and consumer_reference is null)
        or consumer_kind = 'model_provider'
        or (consumer_kind = 'managed_plugin' and consumer_reference is not null)
    );
