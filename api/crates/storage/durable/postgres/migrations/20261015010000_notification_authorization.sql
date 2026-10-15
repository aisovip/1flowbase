-- Host-only authorization state is independent of administrator status and editable config.
-- Managed QR/bot/context/cursor/recipient credentials have a separate encrypted AAD domain.
ALTER TABLE notification_instances
    ADD COLUMN authorization_json jsonb,
    ADD COLUMN encrypted_managed_secret_json jsonb,
    ADD COLUMN authorization_revision bigint NOT NULL DEFAULT 0
        CHECK (authorization_revision >= 0),
    ADD CONSTRAINT notification_authorization_status CHECK (
        authorization_json IS NULL OR (
            jsonb_typeof(authorization_json) = 'object'
            AND authorization_json->>'status' IN ('unauthorized','pending','authorized','invalid')
        )
    );

-- Do not register ciphertext/revision in the generic model catalog. Dedicated notification
-- services are the sole authorization owner and expose only the canonical public view.
