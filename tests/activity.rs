use quick_pr::{
    activity::{Activity, Indicator},
    github::{Preferences, demo},
};

#[test]
fn first_sync_and_unchanged_existing_problems_are_quiet() {
    let snapshot = demo();
    let mut activity = Activity::default();
    activity.observe(&snapshot, &Preferences::default());
    activity.observe(&snapshot, &Preferences::default());
    assert!(activity.pending.is_empty());
    assert_eq!(activity.indicator(), Indicator::Quiet);
}
#[test]
fn new_review_request_is_new_and_acknowledging_clears_the_indicator() {
    let prefs = Preferences::default();
    let original = demo();
    let mut changed = original.clone();
    let mut pr = changed.needs[0].clone();
    pr.id = "new-review".into();
    changed.needs.push(pr);
    let mut activity = Activity::default();
    activity.observe(&original, &prefs);
    activity.observe(&changed, &prefs);
    assert_eq!(activity.indicator(), Indicator::New);
    assert!(activity.pending["new-review"].new);
    assert!(
        activity.pending["new-review"]
            .summary
            .contains("Review requested")
    );
    activity.acknowledge(&changed, &prefs);
    activity.observe(&changed, &prefs);
    assert_eq!(activity.indicator(), Indicator::Quiet);
    assert!(activity.pending.is_empty());
}
#[test]
fn failing_ci_is_red_even_when_other_updates_arrive() {
    let prefs = Preferences::default();
    let original = demo();
    let mut changed = original.clone();
    changed.yours[0].checks = "FAILURE".into();
    changed.needs[0].comments += 1;
    let mut activity = Activity::default();
    activity.observe(&original, &prefs);
    activity.observe(&changed, &prefs);
    assert_eq!(activity.indicator(), Indicator::Problem);
    assert!(!activity.pending["c"].new);
    assert!(activity.pending["c"].summary.contains("Checks failed"));
    assert_eq!(activity.pending.len(), 2);
}
#[test]
fn changes_requested_is_red_without_a_ci_failure() {
    let prefs = Preferences::default();
    let original = demo();
    let mut changed = original.clone();
    changed.yours[0].review = "CHANGES_REQUESTED".into();
    let mut activity = Activity::default();
    activity.observe(&original, &prefs);
    activity.observe(&changed, &prefs);
    assert_eq!(activity.indicator(), Indicator::Problem);
}
#[test]
fn own_ready_pr_is_green_and_takes_priority_over_ordinary_updates() {
    let prefs = Preferences::default();
    let original = demo();
    let mut changed = original.clone();
    changed.yours[0].draft = false;
    changed.yours[0].checks = "SUCCESS".into();
    changed.yours[0].review = "APPROVED".into();
    changed.needs[0].comments += 1;
    let mut activity = Activity::default();
    activity.observe(&original, &prefs);
    activity.observe(&changed, &prefs);
    assert_eq!(activity.indicator(), Indicator::Ready);
    assert!(
        activity.pending["c"]
            .summary
            .contains("Approved with passing checks")
    );
}
#[test]
fn approval_for_someone_elses_pr_is_blue() {
    let prefs = Preferences::default();
    let original = demo();
    let mut changed = original.clone();
    changed.needs[0].author = "another-person".into();
    let original = changed.clone();
    changed.needs[0].review = "APPROVED".into();
    let mut activity = Activity::default();
    activity.observe(&original, &prefs);
    activity.observe(&changed, &prefs);
    assert_eq!(activity.indicator(), Indicator::New);
}
#[test]
fn updates_accumulate_since_last_open_and_repeated_polls_stay_unread() {
    let prefs = Preferences::default();
    let original = demo();
    let mut changed = original.clone();
    let mut activity = Activity::default();
    activity.observe(&original, &prefs);
    changed.needs[0].comments += 1;
    activity.observe(&changed, &prefs);
    changed.yours[0].comments += 1;
    activity.observe(&changed, &prefs);
    activity.observe(&changed, &prefs);
    assert_eq!(activity.pending.len(), 2);
}
#[test]
fn persisted_baseline_preserves_unread_activity_after_restart() {
    let prefs = Preferences::default();
    let original = demo();
    let mut changed = original.clone();
    let mut activity = Activity::default();
    activity.observe(&original, &prefs);
    changed.yours[0].comments += 1;
    activity.observe(&changed, &prefs);
    let mut restored: Activity =
        serde_json::from_str(&serde_json::to_string(&activity).unwrap()).unwrap();
    restored.observe(&changed, &prefs);
    assert_eq!(restored.indicator(), Indicator::New);
    assert_eq!(restored.pending.len(), 1);
}
#[test]
fn account_switch_is_quiet_and_discards_previous_account_activity() {
    let prefs = Preferences::default();
    let original = demo();
    let mut changed = original.clone();
    let mut activity = Activity::default();
    activity.observe(&original, &prefs);
    changed.yours[0].checks = "FAILURE".into();
    activity.observe(&changed, &prefs);
    changed.login = "another-account".into();
    activity.observe(&changed, &prefs);
    assert_eq!(activity.indicator(), Indicator::Quiet);
    assert!(activity.pending.is_empty());
}
#[test]
fn completed_watched_pr_is_updated_and_closed_prs_are_not_ready() {
    let mut original = demo();
    let watched = original.yours[1].clone();
    original.watching = vec![watched.clone()];
    let prefs = Preferences {
        watched: vec![watched.id.clone()],
        ..Default::default()
    };
    let mut activity = Activity::default();
    activity.observe(&original, &prefs);
    let mut changed = original.clone();
    changed.yours.retain(|p| p.id != watched.id);
    changed.watching[0].state = "MERGED".into();
    activity.observe(&changed, &prefs);
    assert_eq!(activity.pending[&watched.id].indicator, Indicator::New);
}

#[test]
fn timestamp_upgrade_is_quiet_but_later_timestamp_changes_are_updates() {
    let prefs = Preferences::default();
    let original = demo();
    let mut changed = original.clone();
    let mut activity = Activity::default();
    activity.observe(&original, &prefs);
    changed.yours[0].updated_at = "2026-11-05T12:00:00Z".into();
    activity.observe(&changed, &prefs);
    assert!(activity.pending.is_empty());
    activity.acknowledge(&changed, &prefs);
    changed.yours[0].updated_at = "2026-11-05T13:00:00Z".into();
    activity.observe(&changed, &prefs);
    assert_eq!(activity.pending["c"].indicator, Indicator::New);
}

#[test]
fn existing_watched_pr_getting_a_personal_review_request_is_an_update() {
    let prefs = Preferences {
        watched: vec!["a".into()],
        ..Default::default()
    };
    let mut original = demo();
    let pr = original.needs.remove(0);
    original.watching.push(pr.clone());
    let mut activity = Activity::default();
    activity.observe(&original, &prefs);
    let mut changed = original.clone();
    changed.needs.push(pr);
    activity.observe(&changed, &prefs);
    assert!(!activity.pending["a"].new);
    assert!(
        activity.pending["a"]
            .summary
            .starts_with("Review requested:")
    );
}
