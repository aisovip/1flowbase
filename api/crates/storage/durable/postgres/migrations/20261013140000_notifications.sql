CREATE TABLE notification_instances (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id),
    type_code text NOT NULL,
    installation_id uuid,
    display_name text NOT NULL,
    status text NOT NULL DEFAULT 'enabled' CHECK (status IN ('enabled', 'disabled')),
    config_json jsonb NOT NULL DEFAULT '{}'::jsonb,
    encrypted_secret_json jsonb,
    configured_secret_fields jsonb NOT NULL DEFAULT '[]'::jsonb,
    is_builtin boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX notification_instances_workspace_idx ON notification_instances(workspace_id, created_at, id);
CREATE UNIQUE INDEX notification_instances_builtin_idx ON notification_instances(workspace_id) WHERE is_builtin;

-- No user / instance foreign keys: historic messages survive either object's deletion.
CREATE TABLE system_notifications (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL,
    instance_id uuid NOT NULL,
    recipient_user_id uuid NOT NULL,
    title text NOT NULL,
    content text NOT NULL,
    read_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX system_notifications_recipient_idx ON system_notifications(workspace_id, recipient_user_id, created_at DESC, id DESC);
CREATE INDEX system_notifications_unread_idx ON system_notifications(workspace_id, recipient_user_id) WHERE read_at IS NULL;

INSERT INTO notification_instances (id, workspace_id, type_code, display_name, is_builtin)
SELECT gen_random_uuid(), id, 'system', 'System notification', true FROM workspaces;

-- Protected catalog metadata only. Dedicated notification services own access; no workspace
-- grant is created and generic API exposure remains disabled. Never register secret ciphertext.
INSERT INTO model_definitions (
    id, scope_kind, scope_id, source_kind, code, title, physical_table_name,
    acl_namespace, audit_namespace, availability_status, status, api_exposure_status,
    owner_kind, is_protected, template_provider, template_code, template_version
) VALUES
    ('00000000-2412-4000-8000-000000000001','system','00000000-0000-0000-0000-000000000000','main_source','notification_instances','Notification instances','notification_instances','state_model.notification_instances','audit.state_model.notification_instances','available','published','published_not_exposed','core',true,'core','general','v1'),
    ('00000000-2412-4000-8000-000000000002','system','00000000-0000-0000-0000-000000000000','main_source','system_notifications','System notifications','system_notifications','state_model.system_notifications','audit.state_model.system_notifications','available','published','published_not_exposed','core',true,'core','general','v1')
ON CONFLICT DO NOTHING;

INSERT INTO model_fields (id,data_model_id,scope_id,code,title,physical_column_name,field_kind,is_system,is_writable,is_required,api_required,is_unique,display_interface,display_options,sort_order,availability_status)
SELECT gen_random_uuid(),m.id,m.scope_id,f.code,f.title,f.code,f.kind,true,false,f.required,false,f.code='id',null,'{}'::jsonb,f.position,'available'
FROM model_definitions m JOIN (VALUES
    ('notification_instances','id','ID','string',true,0),
    ('notification_instances','workspace_id','Workspace ID','many_to_one',true,1),
    ('notification_instances','type_code','Type code','string',true,2),
    ('notification_instances','installation_id','Installation ID','string',false,3),
    ('notification_instances','display_name','Display name','string',true,4),
    ('notification_instances','status','Status','string',true,5),
    ('notification_instances','config_json','Public configuration','json',true,6),
    ('notification_instances','configured_secret_fields','Configured secret fields','json',true,7),
    ('notification_instances','created_at','Created at','datetime',true,8),
    ('notification_instances','updated_at','Updated at','datetime',true,9),
    ('system_notifications','id','ID','string',true,0),
    ('system_notifications','workspace_id','Workspace ID','many_to_one',true,1),
    ('system_notifications','instance_id','Instance ID','string',true,2),
    ('system_notifications','recipient_user_id','Recipient user ID','string',true,3),
    ('system_notifications','title','Title','string',true,4),
    ('system_notifications','content','Content','text',true,5),
    ('system_notifications','read_at','Read at','datetime',false,6),
    ('system_notifications','created_at','Created at','datetime',true,7)
) AS f(model_code,code,title,kind,required,position) ON m.code=f.model_code
WHERE m.scope_kind='system' AND m.scope_id='00000000-0000-0000-0000-000000000000'
ON CONFLICT DO NOTHING;
