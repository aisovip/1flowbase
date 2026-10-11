-- Log ingestion previously ignored api_enabled. Preserve reception for existing
-- log applications; newly created applications retain the default disabled state.
UPDATE applications SET api_enabled = true WHERE application_type = 'agent_logs';
