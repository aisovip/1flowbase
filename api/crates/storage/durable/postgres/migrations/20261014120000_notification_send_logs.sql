-- One row per authorized send invocation; no historical backfill and no actor/instance FK.
CREATE TABLE notification_send_logs (
 id uuid PRIMARY KEY, workspace_id uuid NOT NULL, actor_user_id uuid NOT NULL, instance_id uuid NOT NULL,
 display_name text, type_code text, source text NOT NULL CHECK(source IN ('api','workflow')),
 status text NOT NULL CHECK(status IN ('pending','success','failure','unknown')),
 failure_code text CHECK(failure_code IN ('instance_missing','instance_disabled','type_unavailable','plugin_unavailable','input_invalid','config_invalid','provider_result_unknown','provider_output_invalid')),
 title text, content text, recipient_count bigint CHECK(recipient_count >= 0),
 created_at timestamptz NOT NULL DEFAULT now(), completed_at timestamptz
);
CREATE INDEX notification_send_logs_workspace_idx ON notification_send_logs(workspace_id, created_at DESC,id DESC);
CREATE INDEX notification_send_logs_instance_idx ON notification_send_logs(workspace_id,instance_id,created_at DESC,id DESC);
INSERT INTO model_definitions (id,scope_kind,scope_id,source_kind,code,title,physical_table_name,acl_namespace,audit_namespace,availability_status,status,owner_kind,is_protected,template_provider,template_code,template_version)
VALUES ('00000000-2412-4000-8000-000000000003','system','00000000-0000-0000-0000-000000000000','main_source','notification_send_logs','Notification send logs','notification_send_logs','state_model.notification_send_logs','audit.state_model.notification_send_logs','available','published','core',true,'core','general','v1');
INSERT INTO model_fields (id,data_model_id,scope_id,code,title,physical_column_name,field_kind,is_system,is_writable,is_required,api_required,is_unique,display_interface,display_options,sort_order,availability_status)
SELECT gen_random_uuid(),m.id,m.scope_id,f.code,f.title,f.code,f.kind,true,false,f.required,false,f.code='id',null,'{}'::jsonb,f.position,'available'
FROM model_definitions m JOIN (VALUES
 ('id','ID','string',true,0),('workspace_id','Workspace ID','many_to_one',true,1),('actor_user_id','Actor user ID','string',true,2),('instance_id','Instance ID','string',true,3),
 ('display_name','Display name','string',false,4),('type_code','Type code','string',false,5),('source','Source','string',true,6),('status','Status','string',true,7),
 ('failure_code','Failure code','string',false,8),('title','Title','string',false,9),('content','Content','text',false,10),('recipient_count','Recipient count','number',false,11),
 ('created_at','Created at','datetime',true,12),('completed_at','Completed at','datetime',false,13)
) AS f(code,title,kind,required,position) ON true WHERE m.code='notification_send_logs' AND m.scope_kind='system';
