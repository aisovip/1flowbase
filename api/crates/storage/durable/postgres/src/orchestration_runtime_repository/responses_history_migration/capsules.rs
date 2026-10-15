use super::*;
use crate::provider_protocol_capsule_store::{PgProviderProtocolCapsuleStore, KEY_VERSION};
use crate::secret_crypto::{decrypt_secret_json_with_aad, encrypt_secret_json_with_aad};
use sha2::{Digest, Sha256};

pub(super) fn authenticate(
    flow: &ResponsesHistoryFlowKey,
    slot: &ResponsesHistoryContinuation,
    master_key: &str,
) -> Result<()> {
    anyhow::ensure!(
        slot.key_version == KEY_VERSION,
        "responses_history_capsule_key_version"
    );
    let aad = PgProviderProtocolCapsuleStore::associated_data(
        flow.flow_run_id,
        "continuation",
        &slot.slot_key,
    );
    let plaintext = decrypt_secret_json_with_aad(&slot.encrypted_payload, master_key, &aad)?;
    let bytes = serde_json::to_vec(&plaintext)?;
    anyhow::ensure!(
        plaintext == slot.payload
            && slot.plaintext_size_bytes == i64::try_from(bytes.len())?
            && slot.plaintext_digest
                == format!("sha256:{:x}", base16ct::HexDisplay(&Sha256::digest(&bytes))),
        "responses_history_capsule_integrity"
    );
    Ok(())
}

pub(super) async fn load_all(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    master_key: &str,
    lock: bool,
) -> Result<Vec<ResponsesHistoryContinuation>> {
    let suffix = if lock { "for update" } else { "" };
    let rows=sqlx::query(&format!("select slot_key,encrypted_payload,plaintext_digest,plaintext_size_bytes,key_version,hard_expires_at from provider_protocol_capsules where flow_run_id=$1 and capsule_kind='continuation' order by slot_key {suffix}"))
        .bind(flow.flow_run_id).fetch_all(&mut **tx).await?;
    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
        let slot_key: String = row.try_get("slot_key")?;
        let encrypted_payload: Value = row.try_get("encrypted_payload")?;
        let payload = decrypt_secret_json_with_aad(
            &encrypted_payload,
            master_key,
            &PgProviderProtocolCapsuleStore::associated_data(
                flow.flow_run_id,
                "continuation",
                &slot_key,
            ),
        )?;
        let slot = ResponsesHistoryContinuation {
            slot_key,
            encrypted_payload,
            payload,
            plaintext_digest: row.try_get("plaintext_digest")?,
            plaintext_size_bytes: row.try_get("plaintext_size_bytes")?,
            key_version: row.try_get("key_version")?,
            hard_expires_at: row.try_get("hard_expires_at")?,
        };
        authenticate(flow, &slot, master_key)?;
        result.push(slot);
    }
    Ok(result)
}

async fn replace(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    before: &ResponsesHistoryContinuation,
    after: &ResponsesHistoryContinuation,
) -> Result<()> {
    anyhow::ensure!(
        before.slot_key == after.slot_key
            && before.key_version == after.key_version
            && before.hard_expires_at == after.hard_expires_at,
        "responses_history_capsule_retention_changed"
    );
    let changed=sqlx::query("update provider_protocol_capsules set encrypted_payload=$2,plaintext_digest=$3,plaintext_size_bytes=$4 where flow_run_id=$1 and capsule_kind='continuation' and slot_key=$5 and encrypted_payload=$6 and plaintext_digest=$7 and plaintext_size_bytes=$8 and key_version=$9 and hard_expires_at=$10")
        .bind(flow.flow_run_id).bind(&after.encrypted_payload).bind(&after.plaintext_digest)
        .bind(after.plaintext_size_bytes).bind(&before.slot_key).bind(&before.encrypted_payload)
        .bind(&before.plaintext_digest).bind(before.plaintext_size_bytes).bind(&before.key_version)
        .bind(before.hard_expires_at).execute(&mut **tx).await?;
    anyhow::ensure!(
        changed.rows_affected() == 1,
        "responses_history_capsule_cas_conflict"
    );
    Ok(())
}

pub(super) async fn apply(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    changes: &[ResponsesHistoryContinuationChange],
    master_key: &str,
) -> Result<Vec<ResponsesHistoryContinuationReceipt>> {
    let mut receipts = Vec::with_capacity(changes.len());
    for change in changes {
        authenticate(flow, &change.before, master_key)?;
        let mut after = change.before.clone();
        if change.after_payload != change.before.payload {
            let bytes = serde_json::to_vec(&change.after_payload)?;
            after.encrypted_payload = encrypt_secret_json_with_aad(
                &change.after_payload,
                master_key,
                &PgProviderProtocolCapsuleStore::associated_data(
                    flow.flow_run_id,
                    "continuation",
                    &after.slot_key,
                ),
            )?;
            after.plaintext_digest =
                format!("sha256:{:x}", base16ct::HexDisplay(&Sha256::digest(&bytes)));
            after.plaintext_size_bytes = i64::try_from(bytes.len())?;
            after.payload = change.after_payload.clone();
            replace(tx, flow, &change.before, &after).await?;
        }
        receipts.push(ResponsesHistoryContinuationReceipt {
            before: change.before.clone(),
            after,
        });
    }
    Ok(receipts)
}

pub(super) async fn rollback(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    flow: &ResponsesHistoryFlowKey,
    receipts: &[ResponsesHistoryContinuationReceipt],
    master_key: &str,
) -> Result<()> {
    for receipt in receipts {
        authenticate(flow, &receipt.before, master_key)?;
        authenticate(flow, &receipt.after, master_key)?;
        replace(tx, flow, &receipt.after, &receipt.before).await?;
    }
    Ok(())
}
