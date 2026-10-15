use super::Options;

fn args(extra: &[&str]) -> impl Iterator<Item = String> {
    let mut values = vec![
        "--database",
        "1flowbase",
        "--scope",
        "00000000-0000-0000-0000-000000000001",
        "--application",
        "00000000-0000-0000-0000-000000000002",
        "--output",
        "/tmp/private-migration",
    ];
    values.extend_from_slice(extra);
    values
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>()
        .into_iter()
}

#[test]
fn dry_run_is_default_and_apply_needs_writer_fence() {
    assert!(!Options::parse(args(&[])).unwrap().apply);
    assert!(Options::parse(args(&["--apply"])).is_err());
    assert!(
        Options::parse(args(&["--apply", "--writers-stopped"]))
            .unwrap()
            .apply
    );
}

#[test]
fn pages_have_no_arbitrary_business_upper_bound() {
    assert!(Options::parse(args(&["--page-size", "0"])).is_err());
    assert_eq!(
        Options::parse(args(&["--page-size", "4294967295"]))
            .unwrap()
            .page_size,
        u32::MAX
    );
}

#[test]
fn protected_database_and_duplicate_options_are_rejected() {
    let values = args(&[]).map(|v| {
        if v == "1flowbase" {
            "1flowbase_gateway".into()
        } else {
            v
        }
    });
    assert!(Options::parse(values).is_err());
    assert!(Options::parse(args(&["--database", "other"])).is_err());
}
