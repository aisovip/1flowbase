-- NULL resolves the current default at upload time. Existing fixed bindings stay fixed.
alter table file_tables alter column bound_storage_id drop not null;
