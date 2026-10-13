use extension_contracts::{
    DataModelSystemFieldWritePolicy, DataModelTemplateDescriptor, DataModelTemplateIdentity,
    DataModelTemplateSourceSelector, DataModelTemplateSystemField,
};
use serde_json::json;

pub fn file_template_identity() -> DataModelTemplateIdentity {
    DataModelTemplateIdentity {
        provider: domain::CORE_DATA_MODEL_TEMPLATE_PROVIDER.to_owned(),
        code: domain::file_management::FILE_DATA_MODEL_TEMPLATE_CODE.to_owned(),
        version: domain::file_management::FILE_DATA_MODEL_TEMPLATE_VERSION.to_owned(),
    }
}

pub(crate) fn file_template_descriptor() -> DataModelTemplateDescriptor {
    let mut descriptor = crate::general_data_model_template::general_template_descriptor();
    descriptor.identity = file_template_identity();
    descriptor.source_selector = DataModelTemplateSourceSelector::MainSource;
    descriptor.summary = "File data model".to_owned();
    descriptor.description = "File records backed by object storage. Uploads use the file service; storage metadata is service-owned.".to_owned();
    descriptor
        .operations
        .retain(|operation| operation.permission_action == "view");
    descriptor
        .system_fields
        .extend(domain::file_management::FILE_METADATA_FIELDS.iter().map(
            |(code, title, kind, required)| DataModelTemplateSystemField {
                code: (*code).to_owned(),
                value_schema: match kind {
                    domain::ModelFieldKind::Number => json!({"type":"number","minimum":0}),
                    domain::ModelFieldKind::Json => json!(true),
                    domain::ModelFieldKind::FilePreviewUrl => {
                        json!({"type":["string","null"],"readOnly":true})
                    }
                    _ => json!({"type":"string"}),
                },
                required: *required,
                write_policy: DataModelSystemFieldWritePolicy::RuntimeManaged,
                summary: (*title).to_owned(),
                description: format!("File-service-owned `{code}` metadata."),
            },
        ));
    descriptor
}
