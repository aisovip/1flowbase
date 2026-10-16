use super::*;

#[test]
fn repeated_binding_is_idempotent_and_conflicting_binding_preserves_owner() {
    let mut bindings = ResourceOwnerBindings::default();
    let identity = ("session", 1, 7);
    bindings.bind(identity, "proxy-owner").unwrap();
    bindings.bind(identity, "proxy-owner").unwrap();
    assert_eq!(
        bindings.bind(identity, "direct-owner"),
        Err(ResourceOwnerBindingConflict)
    );
    assert_eq!(bindings.get(&identity), Some(&"proxy-owner"));
}

#[test]
fn full_identity_selects_original_owner_across_generations_and_workers() {
    let mut bindings = ResourceOwnerBindings::default();
    for (identity, owner) in [
        (("session", 1, 7), "proxy-owner"),
        (("session", 2, 7), "direct-owner"),
        (("session", 1, 8), "new-worker-owner"),
        (("another-session", 1, 7), "another-owner"),
    ] {
        bindings.bind(identity, owner).unwrap();
    }
    assert_eq!(bindings.get(&("session", 1, 7)), Some(&"proxy-owner"));
    assert_eq!(bindings.get(&("session", 2, 7)), Some(&"direct-owner"));
    assert_eq!(bindings.get(&("session", 1, 8)), Some(&"new-worker-owner"));
    assert_eq!(bindings.get(&("session", 3, 7)), None);
    assert_eq!(bindings.get(&("session", 1, 9)), None);
}

#[test]
fn retention_is_explicit_and_does_not_affect_other_owners() {
    let mut bindings = ResourceOwnerBindings::default();
    bindings.bind(1, "released").unwrap();
    bindings.bind(2, "active").unwrap();
    bindings.retain(|_, owner| *owner != "released");
    assert_eq!(bindings.get(&1), None);
    assert_eq!(bindings.get(&2), Some(&"active"));
}
