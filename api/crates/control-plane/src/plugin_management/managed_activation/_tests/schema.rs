use super::*;

#[test]
fn pdm_008_malformed_ownership_inventory_fails_closed() {
    let record = ManagedSchemaOwnershipRecord {
        ownership_key: "column:fixture.field".to_string(),
        owner_id: "fixture/managed_schema".to_string(),
        owner_version: "1.0.0".to_string(),
        object_kind: ManagedSchemaObjectKind::OwnedField,
        logical_name: "missing_collection_separator".to_string(),
        physical_table: "fixture".to_string(),
        physical_column: Some("field".to_string()),
        field_type: Some(ManagedSchemaFieldType::String),
        nullable: Some(true),
        active: true,
        plan_fingerprint: "fixture".to_string(),
    };
    assert!(existing_ownership(&record).is_err());
}
