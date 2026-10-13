use super::*;

/// Host supplies native computed values after scoped, authorized repository reads.
#[async_trait]
pub trait RuntimeComputedFieldResolver: Send + Sync {
    async fn project(&self, metadata: &ModelMetadata, records: &mut [Value]) -> Result<()>;
}

impl RuntimeEngine {
    pub fn with_computed_field_resolver(
        mut self,
        resolver: Arc<dyn RuntimeComputedFieldResolver>,
    ) -> Self {
        self.computed_fields = Some(resolver);
        self
    }

    pub(super) async fn project_computed_fields(
        &self,
        metadata: &ModelMetadata,
        records: &mut [Value],
    ) -> Result<()> {
        if !metadata
            .fields
            .iter()
            .any(|field| field.field_kind == domain::ModelFieldKind::FilePreviewUrl)
            || records.is_empty()
        {
            return Ok(());
        }
        self.computed_fields
            .as_ref()
            .ok_or_else(|| anyhow!("native computed field resolver is not configured"))?
            .project(metadata, records)
            .await
    }
}

/// Native projected values cannot be evaluated by a physical repository query.
pub(super) fn validate_computed_query(
    metadata: &ModelMetadata,
    filter: &domain::ResourceFilterExpr,
    sorts: &[RuntimeSortInput],
) -> Result<()> {
    fn validate_filter(
        metadata: &ModelMetadata,
        filter: &domain::ResourceFilterExpr,
    ) -> Result<()> {
        match filter {
            domain::ResourceFilterExpr::All(items) | domain::ResourceFilterExpr::Any(items) => {
                for item in items {
                    validate_filter(metadata, item)?;
                }
            }
            domain::ResourceFilterExpr::Field { field, .. } => validate_field(metadata, field)?,
        }
        Ok(())
    }
    fn validate_field(metadata: &ModelMetadata, code: &str) -> Result<()> {
        if metadata
            .field_by_code(code)
            .is_some_and(|f| f.field_kind == domain::ModelFieldKind::FilePreviewUrl)
        {
            return Err(RuntimeModelError::InvalidOperationField(format!(
                "computed field cannot be sorted or filtered: {code}"
            ))
            .into());
        }
        Ok(())
    }
    validate_filter(metadata, filter)?;
    for sort in sorts {
        validate_field(metadata, &sort.field_code)?;
    }
    Ok(())
}
