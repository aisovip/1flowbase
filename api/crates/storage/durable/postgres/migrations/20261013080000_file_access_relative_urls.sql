-- Only registered main-source file records are affected. Object paths/storage remain unchanged.
do $$
declare file_table record;
begin
    for file_table in
        select tables.id, definitions.id as model_id, definitions.physical_table_name
        from file_tables tables
        join model_definitions definitions on definitions.id = tables.model_definition_id
        where definitions.source_kind = 'main_source'
          and definitions.data_source_instance_id is null
    loop
        if to_regclass(format('%I', file_table.physical_table_name)) is not null then
            execute format('alter table %I alter column path type text, alter column url type text', file_table.physical_table_name);
            execute format('update %I set url = %L || id::text || %L', file_table.physical_table_name,
                '/api/console/files/' || file_table.id::text || '/records/', '/content');
        end if;
        update model_fields set field_kind = 'text'
            where data_model_id = file_table.model_id and code in ('path', 'url') and field_kind = 'string';
        update model_fields set title = 'Access URL'
            where data_model_id = file_table.model_id and code = 'url' and title in ('Cached URL', '缓存地址');
    end loop;
end $$;
