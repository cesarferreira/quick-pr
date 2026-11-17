use quick_pr::{
    github::PullRequest,
    stacks::{groups, navigation},
};
use std::collections::BTreeSet;

fn pr(id: &str, base: &str, head: &str) -> PullRequest {
    PullRequest::from_node(&serde_json::json!({
        "__typename":"PullRequest", "id":id,"title":format!("Change {id}"),
        "url":format!("https://github.com/demo/app/pull/{id}"),"number":1,
        "repository":{"nameWithOwner":"demo/app"},"author":{"login":"demo-user"},
        "baseRefName":base,"headRefName":head,
        "baseRepository":{"nameWithOwner":"demo/app"},"headRepository":{"nameWithOwner":"demo/app"}
    }))
    .unwrap()
}
fn ids(prs: &[PullRequest]) -> Vec<&str> {
    prs.iter().map(|pr| pr.id.as_str()).collect()
}

#[test]
fn branch_metadata_is_decoded_and_old_caches_still_load() {
    let p = pr("1", "main", "feature");
    assert_eq!(p.base_ref, "main");
    assert_eq!(p.head_ref, "feature");
    assert_eq!(p.base_repo, "demo/app");
    assert_eq!(p.head_repo, "demo/app");
    let mut old = serde_json::to_value(p).unwrap();
    for field in ["base_ref", "head_ref", "base_repo", "head_repo"] {
        old.as_object_mut().unwrap().remove(field);
    }
    let p: PullRequest = serde_json::from_value(old).unwrap();
    assert!(p.head_ref.is_empty());
}
#[test]
fn chains_are_grouped_base_first_regardless_of_feed_order() {
    let g = groups(&[
        pr("3", "two", "three"),
        pr("1", "main", "one"),
        pr("2", "one", "two"),
    ]);
    assert_eq!(g.len(), 1);
    assert_eq!(ids(&g[0].prs), ["1", "2", "3"]);
    assert!(g[0].shared_author());
}
#[test]
fn unrelated_prs_targeting_main_stay_separate() {
    let g = groups(&[pr("1", "main", "one"), pr("2", "main", "two")]);
    assert_eq!(g.len(), 2);
    assert!(g.iter().all(|g| !g.is_stack()));
}
#[test]
fn matching_branch_names_in_other_forks_are_not_dependencies() {
    let mut a = pr("1", "main", "one");
    a.head_repo = "other/app".into();
    let b = pr("2", "one", "two");
    assert_eq!(groups(&[a, b]).len(), 2);
}
#[test]
fn branch_names_are_case_sensitive_but_repository_names_are_not() {
    let a = pr("1", "main", "One");
    assert_eq!(groups(&[a.clone(), pr("2", "one", "two")]).len(), 2);
    let mut b = pr("2", "One", "two");
    b.base_repo = "DEMO/App".into();
    assert_eq!(groups(&[a, b]).len(), 1);
}
#[test]
fn branching_cycles_and_ambiguous_heads_are_not_drawn_as_linear_stacks() {
    for prs in [
        vec![
            pr("1", "main", "one"),
            pr("2", "one", "two"),
            pr("3", "one", "three"),
        ],
        vec![pr("1", "two", "one"), pr("2", "one", "two")],
        vec![
            pr("1", "main", "one"),
            pr("2", "main", "one"),
            pr("3", "one", "two"),
        ],
    ] {
        let g = groups(&prs);
        assert_eq!(g.len(), prs.len());
        assert!(g.iter().all(|g| !g.is_stack()));
    }
}
#[test]
fn search_preserves_stack_context_and_mixed_authors_are_not_shared() {
    let a = pr("1", "main", "one");
    let mut b = pr("2", "one", "two");
    b.author = "demo-other".into();
    let g = groups(&[a, b]);
    assert!(!g[0].shared_author());
    assert!(g[0].matches("Change 2"));
    assert!(g[0].matches("@demo-other"));
    assert!(!g[0].matches("missing"));
}
#[test]
fn collapsed_groups_keep_a_keyboard_target_and_correct_scroll_indices() {
    let g = groups(&[
        pr("0", "main", "zero"),
        pr("1", "main", "one"),
        pr("2", "one", "two"),
        pr("3", "two", "three"),
        pr("4", "main", "four"),
    ]);
    let expanded = navigation(&g, &BTreeSet::new());
    assert_eq!(
        expanded.iter().map(|r| r.scroll_index).collect::<Vec<_>>(),
        [0, 2, 3, 4, 5]
    );
    let collapsed = navigation(&g, &BTreeSet::from(["1".to_owned()]));
    assert_eq!(
        collapsed
            .iter()
            .map(|r| r.pr.id.as_str())
            .collect::<Vec<_>>(),
        ["0", "1", "4"]
    );
    assert_eq!(
        collapsed.iter().map(|r| r.scroll_index).collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert!(collapsed[1].collapsed);
}
