use quick_pr::{
    github::demo,
    status_history::{StatusHistory, TimeSource, relative_age},
};

#[test]
fn first_observation_uses_github_activity_without_claiming_a_status_change() {
    let mut snapshot = demo();
    snapshot.saved_at = 2000;
    snapshot.yours[0].updated_at = "1970-01-01T00:25:00Z".into();
    let mut history = StatusHistory::default();
    history.observe(&snapshot);
    let time = history.change_time(&snapshot.yours[0].id).unwrap();
    assert_eq!(time.at, 1500);
    assert_eq!(time.source, TimeSource::GitHubUpdate);
    assert_eq!(
        history.change_time(&snapshot.yours[1].id).unwrap().source,
        TimeSource::FirstSeen
    );
}

#[test]
fn status_clock_persists_across_polls_and_restarts_but_resets_on_ci_change() {
    let mut snapshot = demo();
    snapshot.saved_at = 2000;
    let id = snapshot.yours[0].id.clone();
    let mut history = StatusHistory::default();
    history.observe(&snapshot);
    snapshot.saved_at = 2500;
    snapshot.yours[0].checks = "FAILURE".into();
    history.observe(&snapshot);
    let time = history.change_time(&id).unwrap();
    assert_eq!(time.at, 2500);
    assert_eq!(time.source, TimeSource::StateChange);
    let mut history: StatusHistory =
        serde_json::from_str(&serde_json::to_string(&history).unwrap()).unwrap();
    snapshot.saved_at = 3000;
    snapshot.yours[0].comments += 1;
    snapshot.yours[0].updated_at = "1970-01-01T00:49:00Z".into();
    history.observe(&snapshot);
    assert_eq!(history.change_time(&id).unwrap(), time);
}

#[test]
fn review_draft_and_open_state_changes_all_reset_the_clock() {
    let mut snapshot = demo();
    snapshot.saved_at = 2000;
    let id = snapshot.yours[0].id.clone();
    let mut history = StatusHistory::default();
    history.observe(&snapshot);
    snapshot.saved_at = 2100;
    snapshot.yours[0].review = "APPROVED".into();
    history.observe(&snapshot);
    assert_eq!(history.change_time(&id).unwrap().at, 2100);
    snapshot.saved_at = 2200;
    snapshot.yours[0].draft = !snapshot.yours[0].draft;
    history.observe(&snapshot);
    assert_eq!(history.change_time(&id).unwrap().at, 2200);
    snapshot.saved_at = 2300;
    snapshot.yours[0].state = "CLOSED".into();
    history.observe(&snapshot);
    assert_eq!(history.change_time(&id).unwrap().at, 2300);
}

#[test]
fn old_cached_snapshots_cannot_rewind_a_recorded_change() {
    let mut snapshot = demo();
    snapshot.saved_at = 2000;
    let original = snapshot.clone();
    let id = snapshot.yours[0].id.clone();
    let mut history = StatusHistory::default();
    history.observe(&snapshot);
    snapshot.saved_at = 2500;
    snapshot.yours[0].checks = "FAILURE".into();
    history.observe(&snapshot);
    history.observe(&original);
    assert_eq!(history.change_time(&id).unwrap().at, 2500);
}

#[test]
fn switching_accounts_drops_the_previous_accounts_history() {
    let mut snapshot = demo();
    snapshot.saved_at = 2000;
    let id = snapshot.yours[0].id.clone();
    let mut history = StatusHistory::default();
    history.observe(&snapshot);
    snapshot.saved_at = 3000;
    snapshot.login = "demo-other".into();
    history.observe(&snapshot);
    assert_eq!(history.change_time(&id).unwrap().at, 3000);
    assert_eq!(
        history.change_time(&id).unwrap().source,
        TimeSource::FirstSeen
    );
}

#[test]
fn relative_times_are_compact_and_never_go_negative() {
    assert_eq!(relative_age(1000, 1059), "just now");
    assert_eq!(relative_age(1000, 2500), "25min ago");
    assert_eq!(relative_age(1000, 8200), "2h ago");
    assert_eq!(relative_age(1000, 173800), "2d ago");
    assert_eq!(relative_age(2000, 1000), "just now");
}
