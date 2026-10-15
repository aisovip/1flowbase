-- Authorizable notifications retain the existing send log and its safe failure vocabulary.
-- Preserve every legacy outcome while permitting the new fixed authorization/provider codes.
ALTER TABLE notification_send_logs
    DROP CONSTRAINT notification_send_logs_failure_code_check,
    ADD CONSTRAINT notification_send_logs_failure_code_check CHECK (
        failure_code IN (
            'instance_missing', 'instance_disabled', 'type_unavailable', 'plugin_unavailable',
            'input_invalid', 'config_invalid', 'provider_result_unknown', 'provider_output_invalid',
            'authorization_required', 'authorization_invalid', 'recipient_not_ready', 'provider_rejected'
        )
    );
