use quick_pr::github::*;
use serde_json::json;

fn node() -> serde_json::Value {
    json!({"__typename":"PullRequest", "id":"PR_test", "title":"Fix reconnect", "url":"https://github.com/acme/app/pull/12", "number":12, "repository":{"nameWithOwner":"acme/app"}, "author":null, "isDraft":false, "state":"OPEN", "reviewDecision":"APPROVED", "commits":{"nodes":[{"commit":{"statusCheckRollup":{"state":"SUCCESS"}}}]}, "comments":{"totalCount":2}, "reviews":{"totalCount":1}, "reviewThreads":{"totalCount":3}})
}
#[test]
fn decodes_deleted_author_and_review_discussion() {
    let pr = PullRequest::from_node(&node()).unwrap();
    assert_eq!(pr.author, "deleted user");
    assert_eq!(pr.comments, 6);
    assert_eq!(pr.review_label(), "Approved");
    assert_eq!(pr.checks_label(), "Checks passed");
}
#[test]
fn handles_missing_checks_and_drafts_without_claiming_success() {
    let mut n = node();
    n["commits"] = json!({"nodes":[]});
    n["isDraft"] = json!(true);
    let pr = PullRequest::from_node(&n).unwrap();
    assert_eq!(pr.review_label(), "Draft");
    assert_eq!(pr.checks_label(), "No checks");
}
#[test]
fn rejects_non_pr_nodes_and_unexpected_url_schemes() {
    let mut n = node();
    n["url"] = json!("file:///tmp/test");
    assert!(PullRequest::from_node(&n).is_none());
    n = node();
    n["__typename"] = json!("Issue");
    assert!(PullRequest::from_node(&n).is_none());
}
#[test]
fn merged_status_takes_precedence_over_draft() {
    let mut n = node();
    n["isDraft"] = json!(true);
    n["state"] = json!("MERGED");
    assert_eq!(PullRequest::from_node(&n).unwrap().review_label(), "Merged");
}
#[test]
fn never_notifies_on_first_sync_or_account_switch() {
    let new = demo();
    assert!(changes(&Snapshot::default(), &new).is_empty());
    let mut old = new.clone();
    old.login = "another-user".into();
    assert!(changes(&old, &new).is_empty());
}
#[test]
fn notifications_only_fire_for_transitions_not_unchanged_polls() {
    let old = demo();
    assert!(changes(&old, &old).is_empty());
    let mut new = old.clone();
    new.yours[0].checks = "FAILURE".into();
    let messages = changes(&old, &new);
    assert_eq!(messages.len(), 1);
    assert!(messages[0].starts_with("Checks failed:"));
}
#[test]
fn ready_to_merge_waits_for_both_approval_and_checks() {
    let mut old = demo();
    old.yours[1].review = "REVIEW_REQUIRED".into();
    let mut new = old.clone();
    new.yours[1].review = "APPROVED".into();
    assert!(
        changes(&old, &new)
            .iter()
            .any(|s| s.starts_with("Ready to merge:"))
    );
    new.yours[1].draft = true;
    assert!(changes(&old, &new).is_empty());
}
#[test]
fn no_duplicate_alerts_for_prs_that_are_both_authored_and_watched() {
    let mut old = demo();
    old.watching.push(old.yours[0].clone());
    let mut new = old.clone();
    new.yours[0].comments += 1;
    new.watching[0].comments += 1;
    assert_eq!(changes(&old, &new).len(), 1);
}
#[test]
fn open_at_login_defaults_on_and_stays_off_when_saved_that_way() {
    assert!(Preferences::default().launch_at_login);
    assert!(!Preferences::default().notifications);
    let legacy: Preferences =
        serde_json::from_str(r#"{"watched":["a"],"notifications":true}"#).unwrap();
    assert!(legacy.launch_at_login);
    assert_eq!(legacy.watched, ["a"]);
    let off: Preferences = serde_json::from_str(r#"{"launch_at_login":false}"#).unwrap();
    assert!(!off.launch_at_login);
    assert!(!off.notifications);
}
#[test]
fn login_launch_matches_only_the_open_application_login_item_event() {
    use quick_pr::login_item::{
        LAUNCHED_AS_LOGIN_ITEM, LoginItemAction, OPEN_APPLICATION, PROP_DATA, is_app_bundle,
        is_login_launch, login_item_action,
    };
    assert_eq!(OPEN_APPLICATION, u32::from_be_bytes(*b"oapp"));
    assert_eq!(PROP_DATA, u32::from_be_bytes(*b"prdt"));
    assert_eq!(LAUNCHED_AS_LOGIN_ITEM, u32::from_be_bytes(*b"lgit"));
    assert!(is_login_launch(
        Some(OPEN_APPLICATION),
        Some(LAUNCHED_AS_LOGIN_ITEM)
    ));
    assert!(!is_login_launch(Some(OPEN_APPLICATION), None));
    assert!(!is_login_launch(None, Some(LAUNCHED_AS_LOGIN_ITEM)));
    assert!(!is_login_launch(
        Some(0x7261_7070),
        Some(LAUNCHED_AS_LOGIN_ITEM)
    ));
    assert!(is_app_bundle("/Users/me/Applications/Quick PR.app"));
    assert!(!is_app_bundle("/Users/me/code/quick-pr/target/debug"));
    assert!(!is_app_bundle("/Users/me/Applications/Quick PR.app.bak"));
    assert_eq!(login_item_action(true, 0), LoginItemAction::Register);
    assert_eq!(login_item_action(true, 1), LoginItemAction::Keep);
    assert_eq!(login_item_action(true, 2), LoginItemAction::NeedsApproval);
    assert_eq!(login_item_action(true, 3), LoginItemAction::Register);
    assert_eq!(login_item_action(false, 1), LoginItemAction::Unregister);
    assert_eq!(login_item_action(false, 2), LoginItemAction::Unregister);
    assert_eq!(login_item_action(false, 0), LoginItemAction::Keep);
    assert_eq!(login_item_action(false, 3), LoginItemAction::Keep);
}

#[test]
fn public_demo_uses_only_fictional_accounts_and_repositories() {
    let snapshot = demo();
    assert!(snapshot.login.starts_with("demo-"));
    for pr in snapshot
        .needs
        .iter()
        .chain(&snapshot.yours)
        .chain(&snapshot.watching)
    {
        assert!(pr.author.starts_with("demo-"));
        assert!(pr.repo.starts_with("demo/"));
        assert_eq!(pr.url, "https://github.com/pulls");
    }
}

#[test]
fn author_labels_use_me_only_for_the_signed_in_account() {
    let mut pr = PullRequest::from_node(&node()).unwrap();
    pr.author = "Demo-User".into();
    assert_eq!(pr.author_label("demo-user"), "@me");
    assert_eq!(pr.author_label("someone-else"), "@Demo-User");
    assert_eq!(pr.author_label(""), "@Demo-User");
    pr.author = "deleted user".into();
    assert_eq!(pr.author_label("demo-user"), "deleted user");
}
