//! Offline maintenance journal. A full database archive precedes every write;
//! interrupted commits without a receipt require archive recovery, never guessing.
use anyhow::{ensure, Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
};

pub fn create_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(path)?;
    }
    #[cfg(not(unix))]
    anyhow::bail!("private maintenance journals require Unix permissions");
    File::open(path)?.sync_all()?;
    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    File::open(path.parent().context("journal parent missing")?)?.sync_all()?;
    Ok(())
}

pub fn backup_database(directory: &Path, database_url: &str) -> Result<()> {
    let archive = directory.join("before.dump");
    let status = Command::new("pg_dump")
        .env("PGDATABASE", database_url)
        .arg("--format=custom")
        .arg("--file")
        .arg(&archive)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("pg_dump must be installed before apply")?;
    ensure!(
        status.success(),
        "database backup failed; no migration writes started"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&archive, fs::Permissions::from_mode(0o600))?;
    }
    let mut file = File::open(&archive)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let size = file.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        digest.update(&buffer[..size]);
    }
    file.sync_all()?;
    write_json(
        &directory.join("backup.json"),
        &serde_json::json!({
            "archive": "before.dump", "sha256": format!("{:x}", base16ct::HexDisplay(&digest.finalize())),
            "bytes": file.metadata()?.len(),
            "rollback_semantics": "Rollback restores effective recovered data by appending an original-content restoration head. Original latest context ID or absence is not restored; immutable provenance remains.",
        "recovery": "Keep all writers stopped. If an intent lacks a receipt after interruption, restore the complete archive before retrying; never restart the single-format runtime over partial migration."
        }),
    )
}
