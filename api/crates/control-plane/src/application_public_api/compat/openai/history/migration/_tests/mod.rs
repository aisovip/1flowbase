use super::*;
use serde_json::json;

fn items() -> Vec<Value> {
    vec![
        json!({"role":"user","content":"line\n\"雪\""}),
        json!({"type":"future_item","opaque":{"z":1,"a":"x"}}),
        json!({"type":"reasoning","encrypted_content":"opaque-cipher","summary":[]}),
        json!({"type":"function_call","id":"fc_old","call_id":"call_1","name":"exec","arguments":"{ \"x\": 1 }"}),
    ]
}

// Reconstruct a genuine persisted proof using the frozen original algorithm and
// original domain bytes, independently of the accumulator's append/verify path.
fn frozen_proof(version: u32, items: &[Value]) -> Value {
    let mut digest = format!(
        "{:x}",
        Sha256::digest(format!("1flowbase.responses.history.v{version}"))
    );
    for item in items {
        let mut normalized = match version {
            1 => legacy::normalize_item(item).unwrap(),
            2 => legacy_v2::normalize_item(item).unwrap(),
            _ => panic!("test fixture requires a legacy format"),
        };
        normalized.sort_all_objects();
        let bytes = serde_json::to_vec(&normalized).unwrap();
        let mut hash = Sha256::new();
        hash.update(format!("1flowbase.responses.history.item.v{version}\0"));
        hash.update(digest.as_bytes());
        hash.update((bytes.len() as u64).to_be_bytes());
        hash.update(bytes);
        digest = format!("{:x}", hash.finalize());
    }
    json!({"version":version,"item_count":items.len(),"digest":digest})
}

fn accumulate(version: u32, items: &[Value]) -> MigrationAccumulator {
    let mut accumulator = MigrationAccumulator::new(version).unwrap();
    for item in items {
        accumulator.append(item).unwrap();
    }
    accumulator
}

#[test]
fn frozen_v2_nested_wire_vector_is_unchanged() {
    let items = &items()[..2];
    let expected = json!({"version":2,"item_count":2,
        "digest":"a233467c7b28f741f92a18af8cf1e9608b9b2562e436a7a10f4fe521a50924f7"});
    assert_eq!(frozen_proof(2, items), expected);
    assert_eq!(
        accumulate(2, items).verify(&expected).unwrap().old(),
        expected
    );
}

#[test]
fn frozen_v1_nested_wire_vector_is_unchanged() {
    let items = &items()[..2];
    let expected = json!({"version":1,"item_count":2,
        "digest":"ed3369f82a4f9ea679c8e3adc76d72e74488c0e6a9b5d5e611acb776ddc51c83"});
    assert_eq!(frozen_proof(1, items), expected);
    assert_eq!(
        accumulate(1, items).verify(&expected).unwrap().old(),
        expected
    );
}

#[test]
fn complete_legacy_proofs_produce_the_normal_current_generator_output() {
    let items = items();
    let current = super::super::completed_history(&json!({"input":items}), None, &[])
        .unwrap()
        .unwrap();
    for version in [1, 2] {
        let expected = frozen_proof(version, &items);
        let accumulator = accumulate(version, &items);
        let progress = accumulator.snapshot();
        assert_eq!(progress.old(), expected);
        assert_eq!(progress.current(), current);
        let receipt = accumulator.verify(&expected).unwrap();
        assert_eq!(receipt.old(), expected);
        assert_eq!(receipt.current(), current);
        assert_eq!(
            serde_json::to_value(&receipt).unwrap(),
            json!({"old":expected,"current":current})
        );
        assert_ne!(receipt.old()["digest"], receipt.current()["digest"]);
    }
}

#[test]
fn verification_rejects_partial_reordered_and_changed_semantic_evidence() {
    let items = items();
    for version in [1, 2] {
        let expected = frozen_proof(version, &items);
        assert!(accumulate(version, &items[..3]).verify(&expected).is_err());
        let mut reordered = items.clone();
        reordered.swap(0, 1);
        assert!(accumulate(version, &reordered).verify(&expected).is_err());
        for (index, path, replacement) in [
            (1, vec!["opaque", "z"], json!(1.0)),
            (1, vec!["opaque", "z"], json!(2)),
            (2, vec!["encrypted_content"], json!("changed-cipher")),
            (3, vec!["arguments"], json!("{\"x\":1}")),
            (3, vec!["call_id"], json!("foreign_call")),
        ] {
            let mut changed = items.clone();
            let mut value = &mut changed[index];
            for key in path {
                value = &mut value[key];
            }
            *value = replacement;
            assert!(accumulate(version, &changed).verify(&expected).is_err());
        }
        let mut longer = items.clone();
        longer.push(json!({"role":"user","content":"extra"}));
        assert!(accumulate(version, &longer).verify(&expected).is_err());
    }
}

#[test]
fn malformed_or_mismatched_proofs_are_never_receipts() {
    let items = items();
    let accumulator = accumulate(2, &items);
    let expected = frozen_proof(2, &items);
    for version in [
        json!(0),
        json!(3),
        json!(999),
        json!("2"),
        json!(2.0),
        json!(-1),
        Value::Null,
    ] {
        let mut malformed = expected.clone();
        malformed["version"] = version;
        assert!(accumulator.verify(&malformed).is_err());
    }
    assert!(accumulator.verify(&frozen_proof(1, &items)).is_err());
    for field in ["version", "item_count", "digest"] {
        let mut missing = expected.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(accumulator.verify(&missing).is_err());
    }
    for count in [json!(0), json!(3), json!(5), json!(-1), json!("4")] {
        let mut malformed = expected.clone();
        malformed["item_count"] = count;
        assert!(accumulator.verify(&malformed).is_err());
    }
    for digest in [
        json!("bad"),
        json!("z".repeat(64)),
        json!("0".repeat(64)),
        Value::Null,
    ] {
        let mut malformed = expected.clone();
        malformed["digest"] = digest;
        assert!(accumulator.verify(&malformed).is_err());
    }
    let mut unknown = expected.clone();
    unknown["extra"] = json!(true);
    assert!(accumulator.verify(&unknown).is_err());
    assert!(accumulator.verify(&Value::Null).is_err());
    for version in [0, 3, 999] {
        assert!(MigrationAccumulator::new(version).is_err());
    }
}

#[test]
fn verified_predecessors_and_source_state_clones_preserve_both_chains() {
    let items = items();
    for version in [1, 2] {
        let prefix = accumulate(version, &items[..2]);
        let receipt = prefix.verify(&frozen_proof(version, &items[..2])).unwrap();
        let mut continued = receipt.continue_history();
        let mut branch = prefix.clone();
        for item in &items[2..] {
            continued.append(item).unwrap();
            branch.append(item).unwrap();
        }
        let expected = frozen_proof(version, &items);
        let whole = accumulate(version, &items).verify(&expected).unwrap();
        for result in [
            continued.verify(&expected).unwrap(),
            branch.verify(&expected).unwrap(),
        ] {
            assert_eq!(result.old(), whole.old());
            assert_eq!(result.current(), whole.current());
        }
        assert_eq!(prefix.snapshot().old()["item_count"], 2);
        assert!(prefix.verify(&expected).is_err());
    }
}

#[test]
fn invalid_items_leave_both_progress_hashes_unchanged() {
    for version in [1, 2] {
        let mut accumulator = accumulate(version, &items());
        let before = serde_json::to_value(accumulator.snapshot()).unwrap();
        // V1 accepts object types the current semantics reject; this exercises
        // atomicity after successful legacy normalization as well.
        for invalid in [json!(null), json!(42), json!({"type":42})] {
            assert!(accumulator.append(&invalid).is_err());
            assert_eq!(
                serde_json::to_value(accumulator.snapshot()).unwrap(),
                before
            );
        }
    }
}

#[test]
fn frozen_v2_delivery_ids_remain_evidence_before_migration() {
    let items = items();
    let expected = frozen_proof(2, &items);
    let mut without_id = items.clone();
    without_id[3].as_object_mut().unwrap().remove("id");
    assert!(accumulate(2, &without_id).verify(&expected).is_err());
    let original = accumulate(2, &items).verify(&expected).unwrap();
    let changed = accumulate(2, &without_id)
        .verify(&frozen_proof(2, &without_id))
        .unwrap();
    assert_eq!(original.current(), changed.current());
}

#[test]
fn frozen_legacy_contracts_keep_their_original_tracking_difference() {
    let items = [json!({"type":"message","role":"user","content":"hello",
        "metadata":{"turn_id":"original-delivery"}})];
    let mut changed = items.clone();
    changed[0]["metadata"]["turn_id"] = json!("different-delivery");
    let v1 = frozen_proof(1, &items);
    let v2 = frozen_proof(2, &items);
    assert!(accumulate(1, &changed).verify(&v1).is_err());
    let receipt = accumulate(2, &changed).verify(&v2).unwrap();
    assert_eq!(
        receipt.current(),
        accumulate(1, &items).verify(&v1).unwrap().current()
    );
}

#[test]
fn verified_empty_prewarm_can_seed_a_complete_later_chain() {
    for version in [1, 2] {
        let empty = MigrationAccumulator::new(version).unwrap();
        let receipt = empty.verify(&frozen_proof(version, &[])).unwrap();
        assert_eq!(
            receipt.current(),
            super::super::completed_history(&json!({"input":[]}), None, &[])
                .unwrap()
                .unwrap()
        );
        let mut continued = receipt.continue_history();
        let items = items();
        for item in &items {
            continued.append(item).unwrap();
        }
        assert_eq!(
            continued
                .verify(&frozen_proof(version, &items))
                .unwrap()
                .current(),
            accumulate(version, &items)
                .verify(&frozen_proof(version, &items))
                .unwrap()
                .current()
        );
    }
}
