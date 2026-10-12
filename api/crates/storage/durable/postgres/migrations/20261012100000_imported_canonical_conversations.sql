
-- Imported ownership is independent of the reception API key. Raw observations
-- and task/source identities are unchanged; unresolved declarations stay unowned.
alter table application_conversations add column source_kind text not null default 'native';
alter table application_conversations add column source_id text;
alter table application_conversations add column source_client text;
alter table application_conversations add column source_session_id text;
create unique index application_conversations_imported_identity
on application_conversations(application_id,scope_id,source_id,source_client,source_session_id)
where source_kind='imported';

create function refresh_imported_conversation_ownership(
    requested_application uuid, requested_scope uuid,
    requested_source text, requested_client text
) returns void language plpgsql as $$
begin
    insert into application_conversations(
        id,application_id,scope_id,external_conversation_id,title,
        source_kind,source_id,source_client,source_session_id,created_at,updated_at)
    select gen_random_uuid(),
        t.application_id,t.scope_id,t.source_session_id,
        (array_agg(coalesce(nullif(t.user_input,''),t.title)
            order by (nullif(t.user_input,'') is null),t.started_at,t.id))[1],
        'imported',t.source_id,t.source_client,t.source_session_id,min(t.created_at),max(t.updated_at)
    from application_run_log_tasks t
    where t.source_kind='imported' and t.parent_task_run_id is null and t.parent_source_task_id is null
        and nullif(t.source_id,'') is not null and nullif(t.source_client,'') is not null
        and nullif(t.source_session_id,'') is not null
        and (requested_application is null or t.application_id=requested_application)
        and (requested_scope is null or t.scope_id=requested_scope)
        and (requested_source is null or t.source_id=requested_source)
        and (requested_client is null or t.source_client=requested_client)
    group by t.application_id,t.scope_id,t.source_id,t.source_client,t.source_session_id
    on conflict (application_id,scope_id,source_id,source_client,source_session_id)
        where source_kind='imported'
    do update set title=excluded.title,
        created_at=least(application_conversations.created_at,excluded.created_at),
        updated_at=greatest(application_conversations.updated_at,excluded.updated_at);

    with recursive owners as (
        select t.id,t.application_id,t.scope_id,t.source_id,t.source_client,c.id as conversation_id,array[t.id] as path
        from application_run_log_tasks t join application_conversations c
          on c.application_id=t.application_id and c.scope_id=t.scope_id and c.source_kind=t.source_kind
          and c.source_id=t.source_id and c.source_client=t.source_client and c.source_session_id=t.source_session_id
        where t.source_kind='imported' and t.parent_task_run_id is null and t.parent_source_task_id is null
          and (requested_application is null or t.application_id=requested_application)
          and (requested_scope is null or t.scope_id=requested_scope)
          and (requested_source is null or t.source_id=requested_source)
          and (requested_client is null or t.source_client=requested_client)
        union all
        select child.id,o.application_id,o.scope_id,o.source_id,o.source_client,o.conversation_id,o.path||child.id
        from owners o join application_run_log_tasks child on child.parent_task_run_id=o.id
          and child.application_id=o.application_id and child.scope_id=o.scope_id and child.source_kind='imported'
          and child.source_id=o.source_id and child.source_client=o.source_client
        where not child.id=any(o.path)
    ), desired as (
        select t.id,o.conversation_id from application_run_log_tasks t left join owners o on o.id=t.id
        where t.source_kind='imported'
          and (requested_application is null or t.application_id=requested_application)
          and (requested_scope is null or t.scope_id=requested_scope)
          and (requested_source is null or t.source_id=requested_source)
          and (requested_client is null or t.source_client=requested_client)
    )
    update application_run_log_tasks t set log_conversation_id=d.conversation_id
    from desired d where t.id=d.id and t.log_conversation_id is distinct from d.conversation_id;
end $$;
select refresh_imported_conversation_ownership(null,null,null,null);

-- Expose persisted namespace identity through the existing typed filter reader.
insert into model_fields (id,data_model_id,scope_id,code,title,physical_column_name,field_kind,is_system,is_writable,is_required,api_required,is_unique,display_options,relation_options,sort_order,availability_status)
select gen_random_uuid(),m.id,m.scope_id,'source_id','Source ID','source_id','string',true,false,false,false,false,'{}','{}',126,'available'
from model_definitions m where m.code='application_run_log_tasks' and m.scope_kind='system'
and not exists(select 1 from model_fields old where old.data_model_id=m.id and old.code='source_id');

-- Canonical imported identity belongs to the runtime read contract. Insert
-- missing metadata only; existing titles, descriptions, display options and
-- exposure/grant preferences remain user-owned.
insert into model_fields (id,data_model_id,scope_id,code,title,physical_column_name,field_kind,is_system,is_writable,is_required,api_required,is_unique,display_options,relation_options,sort_order,availability_status)
select gen_random_uuid(),m.id,m.scope_id,f.code,f.title,f.code,'string',true,false,f.required,f.required,false,'{}','{}',f.position,'available'
from model_definitions m cross join (values
    ('source_kind', 'Source kind', true, 9),
    ('source_id', 'Source ID', false, 10),
    ('source_client', 'Source client', false, 11),
    ('source_session_id', 'Source session ID', false, 12)
) f(code,title,required,position)
where m.code='application_conversations' and m.scope_kind='system'
and not exists(select 1 from model_fields old where old.data_model_id=m.id and old.code=f.code);
