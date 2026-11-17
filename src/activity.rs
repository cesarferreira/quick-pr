use crate::github::{Preferences, PullRequest, Snapshot};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// Declaration order defines the menu bar priority: problems, ready, ordinary updates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Indicator {
    #[default]
    Quiet,
    New,
    Ready,
    Problem,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub new: bool,
    pub indicator: Indicator,
    pub summary: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    pr: PullRequest,
    requested: bool,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Activity {
    login: String,
    initialized: bool,
    seen: BTreeMap<String, Entry>,
    // Rebuilt from the last-opened baseline and cached snapshot on restart.
    #[serde(skip)]
    pub pending: BTreeMap<String, Change>,
}

impl Activity {
    pub fn observe(&mut self, snapshot: &Snapshot, prefs: &Preferences) {
        if snapshot.saved_at == 0 || snapshot.login.is_empty() {
            return;
        }
        if !self.initialized || !self.login.eq_ignore_ascii_case(&snapshot.login) {
            self.acknowledge(snapshot, prefs);
            return;
        }
        self.pending = tracked(snapshot, prefs)
            .into_iter()
            .filter_map(|(id, entry)| {
                let previous = self.seen.get(&id);
                if previous.is_some_and(|old| unchanged(old, &entry)) {
                    return None;
                }
                let pr = &entry.pr;
                let own = pr.author.eq_ignore_ascii_case(&snapshot.login);
                let problem = pr.state == "OPEN"
                    && (matches!(pr.checks.as_str(), "FAILURE" | "ERROR")
                        || pr.review == "CHANGES_REQUESTED");
                let became_ready = own && ready(pr) && previous.is_none_or(|old| !ready(&old.pr));
                let indicator = if problem {
                    Indicator::Problem
                } else if became_ready {
                    Indicator::Ready
                } else {
                    Indicator::New
                };
                let reason = if pr.state != "OPEN" {
                    pr.review_label().to_owned()
                } else if problem && matches!(pr.checks.as_str(), "FAILURE" | "ERROR") {
                    "Checks failed".into()
                } else if problem {
                    "Changes requested".into()
                } else if became_ready {
                    "Approved with passing checks".into()
                } else if entry.requested && previous.is_none_or(|old| !old.requested) {
                    "Review requested".into()
                } else if previous.is_none() {
                    "New PR".into()
                } else {
                    let old = &previous.unwrap().pr;
                    if pr.checks != old.checks {
                        pr.checks_label().into()
                    } else if pr.review != old.review || pr.draft != old.draft {
                        pr.review_label().into()
                    } else if pr.comments > old.comments {
                        "New discussion".into()
                    } else {
                        "PR updated".into()
                    }
                };
                let summary = format!("{reason}: {}", pr.title);
                Some((
                    id,
                    Change {
                        new: previous.is_none(),
                        indicator,
                        summary,
                    },
                ))
            })
            .collect();
    }

    pub fn acknowledge(&mut self, snapshot: &Snapshot, prefs: &Preferences) {
        if snapshot.saved_at == 0 || snapshot.login.is_empty() {
            return;
        }
        self.login = snapshot.login.clone();
        self.seen = tracked(snapshot, prefs);
        self.initialized = true;
        self.pending.clear();
    }

    pub fn indicator(&self) -> Indicator {
        self.pending
            .values()
            .map(|change| change.indicator)
            .max()
            .unwrap_or_default()
    }

    pub fn tooltip(&self) -> String {
        if self.pending.is_empty() {
            return "Quick PR — You’re caught up · ⌃⌥P".into();
        }
        let mut changes: Vec<_> = self.pending.values().collect();
        changes.sort_by_key(|change| std::cmp::Reverse(change.indicator));
        let details = changes
            .iter()
            .take(3)
            .map(|change| change.summary.chars().take(160).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        let extra = if changes.len() > 3 {
            format!("\n…and {} more", changes.len() - 3)
        } else {
            String::new()
        };
        format!(
            "Quick PR — {} update{} since last opened\n{details}{extra}\n⌃⌥P to view",
            changes.len(),
            if changes.len() == 1 { "" } else { "s" }
        )
    }
}

fn ready(pr: &PullRequest) -> bool {
    pr.state == "OPEN" && !pr.draft && pr.review == "APPROVED" && pr.checks == "SUCCESS"
}

fn unchanged(old: &Entry, current: &Entry) -> bool {
    // The pre-activity cache had no updatedAt field; don't treat schema migration
    // as new GitHub activity. Status differences are still detected normally.
    let mut previous = old.pr.clone();
    if previous.updated_at.is_empty() {
        previous.updated_at = current.pr.updated_at.clone();
    }
    // Older caches predate branch metadata; its arrival is not new activity.
    if previous.base_ref.is_empty() && previous.head_ref.is_empty() {
        previous.base_ref = current.pr.base_ref.clone();
        previous.head_ref = current.pr.head_ref.clone();
        previous.base_repo = current.pr.base_repo.clone();
        previous.head_repo = current.pr.head_repo.clone();
    }
    previous == current.pr && old.requested == current.requested
}

fn tracked(snapshot: &Snapshot, prefs: &Preferences) -> BTreeMap<String, Entry> {
    let mut entries = BTreeMap::new();
    for pr in snapshot.relevant(prefs).iter().chain(
        snapshot
            .watching
            .iter()
            .filter(|pr| prefs.watched.contains(&pr.id)),
    ) {
        entries.entry(pr.id.clone()).or_insert_with(|| Entry {
            pr: pr.clone(),
            requested: snapshot.needs.iter().any(|p| p.id == pr.id),
        });
    }
    entries
}
