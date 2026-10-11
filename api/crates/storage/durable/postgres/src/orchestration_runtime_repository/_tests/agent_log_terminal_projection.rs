use super::observed_task_settlement;

#[test]
fn agent_log_terminal_failure_and_cancel_never_become_success() {
    for (phase, status) in [
        ("error", "failed"),
        ("failed", "failed"),
        ("cancelled", "cancelled"),
        ("aborted", "cancelled"),
    ] {
        assert_eq!(
            observed_task_settlement(Some((Some(phase), "terminal-time")), None),
            (status, "no_final_answer", Some("terminal-time"))
        );
        // A real answer observation cannot override a later driver failure.
        assert_eq!(
            observed_task_settlement(Some((Some(phase), "terminal-time")), Some("answer-time")),
            (status, "final_answer_observed", Some("terminal-time"))
        );
    }
}

#[test]
fn agent_log_terminal_success_phases_preserve_existing_clients() {
    for phase in [
        None,
        Some("completed"),
        Some("final_answer"),
        Some("end_turn"),
        Some("stop_sequence"),
        Some("turn_duration"),
        Some("stop"),
    ] {
        assert_eq!(
            observed_task_settlement(Some((phase, "terminal-time")), Some("answer-time")),
            ("succeeded", "final_answer_observed", Some("terminal-time"))
        );
        assert_eq!(
            observed_task_settlement(Some((phase, "terminal-time")), None),
            ("succeeded", "no_final_answer", Some("terminal-time"))
        );
    }
}

#[test]
fn agent_log_terminal_incomplete_and_unknown_reasons_do_not_fabricate_success() {
    for phase in [
        "max-tokens",
        "blocked",
        "interrupted",
        "forked",
        "future-terminal",
    ] {
        assert_eq!(
            observed_task_settlement(Some((Some(phase), "terminal-time")), None),
            ("incomplete", "no_final_answer", Some("terminal-time"))
        );
    }
    assert_eq!(
        observed_task_settlement(None, None),
        ("running", "in_progress", None)
    );
    assert_eq!(
        observed_task_settlement(None, Some("answer-time")),
        ("succeeded", "final_answer_observed", Some("answer-time"))
    );
}
