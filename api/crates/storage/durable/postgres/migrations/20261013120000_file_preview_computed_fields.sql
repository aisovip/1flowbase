-- Declare native computed metadata for existing file models; do not create a physical column.
insert into model_fields (id, data_model_id, scope_id, code, title, physical_column_name,
    field_kind, is_system, is_writable, is_required, api_required, is_unique, display_options, relation_options, sort_order)
select gen_random_uuid(), definitions.id, definitions.scope_id, 'preview_url', 'Image Preview URL', '',
    'file_preview_url', true, false, false, false, false, '{}'::jsonb, '{}'::jsonb,
    coalesce((select max(fields.sort_order) + 1 from model_fields fields where fields.data_model_id = definitions.id), 0)
from model_definitions definitions
where definitions.source_kind = 'main_source'
    and exists (select 1 from file_tables where model_definition_id = definitions.id)
on conflict (data_model_id, code) do nothing;

update file_storages set rule_json = rule_json || '{"image_preview_enabled":true}'::jsonb
where driver_type = 'local' and not (rule_json ? 'image_preview_enabled');
