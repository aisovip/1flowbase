use anyhow::{bail, ensure, Context, Result};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
use uuid::Uuid;

pub struct Options {
    pub database: String,
    pub scope: Uuid,
    pub application: Uuid,
    pub output: PathBuf,
    pub page_size: u32,
    pub apply: bool,
    pub excluded_flows: BTreeSet<Uuid>,
}

impl Options {
    pub fn parse(args: impl Iterator<Item = String>) -> Result<Self> {
        let mut values = BTreeMap::new();
        let mut apply = false;
        let mut stopped = false;
        let mut excluded_flows = BTreeSet::new();
        let mut args = args;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--exclude-flow" => {
                    let flow = args.next().context("missing excluded flow UUID")?.parse()?;
                    ensure!(excluded_flows.insert(flow), "duplicate excluded flow");
                }
                "--apply" => { ensure!(!apply, "duplicate --apply"); apply = true; }
                "--writers-stopped" => { ensure!(!stopped, "duplicate writer fence"); stopped = true; }
                "--database" | "--scope" | "--application" | "--output" | "--page-size" => {
                    let value = args.next().context("missing option value")?;
                    ensure!(values.insert(arg, value).is_none(), "duplicate option");
                }
                _ => bail!("unknown option; required: --database NAME --scope UUID --application UUID --output NEW_PRIVATE_DIRECTORY; optional --page-size N, repeatable --exclude-flow UUID (preserves flow unchanged); default dry-run; writes require --apply --writers-stopped"),
            }
        }
        let database = values.remove("--database").context("--database required")?;
        ensure!(
            database != "1flowbase_gateway",
            "protected gateway database"
        );
        ensure!(
            !apply || stopped,
            "apply requires stopped writers and --writers-stopped"
        );
        let page_size = values
            .remove("--page-size")
            .unwrap_or_else(|| "128".into())
            .parse()?;
        ensure!(page_size > 0, "page size must be positive");
        Ok(Self {
            database,
            scope: values
                .remove("--scope")
                .context("--scope required")?
                .parse()?,
            application: values
                .remove("--application")
                .context("--application required")?
                .parse()?,
            output: values
                .remove("--output")
                .context("--output required")?
                .into(),
            page_size,
            apply,
            excluded_flows,
        })
    }

    /// Exclusions are explicit scope selections, never a fallback for missing proof.
    pub fn validate_exclusions(&self, scoped_flows: impl Iterator<Item = Uuid>) -> Result<()> {
        let scoped: BTreeSet<_> = scoped_flows.collect();
        ensure!(
            self.excluded_flows.is_subset(&scoped),
            "excluded flow is outside enumerated scope/application"
        );
        Ok(())
    }
}

#[cfg(test)]
#[path = "_tests/options.rs"]
mod tests;
