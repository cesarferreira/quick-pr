use quick_pr::github::*;

#[test]
fn discoveries_only_include_recent_authored_prs_and_personal_review_requests() {
    // 2026-11-03 00:00 UTC; the cutoff is 2026-10-20.
    let (mine, needs) = relevance_queries("alice", 1793664000);
    assert!(mine.contains("author:alice"));
    assert!(
        needs
            .split_whitespace()
            .any(|q| q == "user-review-requested:alice")
    );
    for query in [mine, needs] {
        assert!(query.contains("is:open"));
        assert!(query.contains("updated:>=2026-10-20"));
    }
}

#[test]
fn relevant_feed_combines_owned_direct_and_explicit_watches_without_duplicates() {
    let mut snapshot = demo();
    snapshot.needs.truncate(1);
    snapshot.yours.truncate(1);
    let mut watched = snapshot.needs[0].clone();
    watched.id = "watched".into();
    let mut closed = watched.clone();
    closed.id = "closed".into();
    closed.state = "MERGED".into();
    let prefs = Preferences {
        watched: vec![
            watched.id.clone(),
            closed.id.clone(),
            snapshot.yours[0].id.clone(),
        ],
        ..Default::default()
    };
    snapshot.watching = vec![watched, closed, snapshot.yours[0].clone()];
    let ids: Vec<_> = snapshot
        .relevant(&prefs)
        .into_iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(ids, vec!["a", "c", "watched"]);
}

#[test]
fn unwatching_removes_a_pr_from_relevant_even_if_the_old_snapshot_still_has_it() {
    let mut snapshot = demo();
    snapshot.needs.clear();
    snapshot.yours.clear();
    snapshot.watching = demo().needs;
    assert!(snapshot.relevant(&Preferences::default()).is_empty());
}

#[test]
fn old_broad_cache_cannot_reintroduce_team_requests_before_refresh() {
    let legacy = serde_json::json!({"login":"alice","needs":[{"id":"team-noise", "title":"Unrelated", "url":"https://github.com/a/b/pull/1", "number":1,"repo":"a/b","author":"bob","draft":false,"state":"OPEN","review":"","checks":"","comments":0}],"yours":[],"watching":[],"saved_at":123,"remaining":4999,"reset_at":"","truncated":false});
    let snapshot: Snapshot = serde_json::from_value(legacy).unwrap();
    let cleaned = snapshot.current_or_empty();
    assert!(cleaned.needs.is_empty());
    assert_eq!(cleaned.saved_at, 0);
}
