use crate::github::{PullRequest, Snapshot};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeSource {
    StateChange,
    GitHubUpdate,
    FirstSeen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeTime {
    pub at: u64,
    pub source: TimeSource,
}

impl ChangeTime {
    pub fn tooltip(self) -> String {
        let description = match self.source {
            TimeSource::StateChange => "Review, CI, draft, or PR state change observed",
            TimeSource::GitHubUpdate => {
                "GitHub activity before status tracking began; status-change time is unknown"
            }
            TimeSource::FirstSeen => {
                "First seen by Quick PR; earlier status-change time is unknown"
            }
        };
        let absolute = i64::try_from(self.at)
            .ok()
            .and_then(|at| chrono::DateTime::from_timestamp(at, 0))
            .map(|time| time.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_default();
        format!("{description}\n{absolute}")
    }
}

#[derive(PartialEq, Eq, Serialize, Deserialize)]
struct Status {
    state: String,
    review: String,
    checks: String,
    draft: bool,
}

impl From<&PullRequest> for Status {
    fn from(pr: &PullRequest) -> Self {
        Self {
            state: pr.state.clone(),
            review: pr.review.clone(),
            checks: pr.checks.clone(),
            draft: pr.draft,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Entry {
    status: Status,
    time: ChangeTime,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StatusHistory {
    login: String,
    last_observed_at: u64,
    entries: BTreeMap<String, Entry>,
}

impl StatusHistory {
    pub fn observe(&mut self, snapshot: &Snapshot) {
        if snapshot.saved_at == 0 || snapshot.login.is_empty() {
            return;
        }
        if !self.login.eq_ignore_ascii_case(&snapshot.login) {
            self.entries.clear();
            self.last_observed_at = 0;
            self.login = snapshot.login.clone();
        }
        if snapshot.saved_at < self.last_observed_at {
            return;
        }
        let mut present = BTreeSet::new();
        for pr in snapshot
            .needs
            .iter()
            .chain(&snapshot.yours)
            .chain(&snapshot.watching)
        {
            if !present.insert(pr.id.clone()) {
                continue;
            }
            let status = Status::from(pr);
            match self.entries.get_mut(&pr.id) {
                Some(entry) if entry.status != status => {
                    *entry = Entry {
                        status,
                        time: ChangeTime {
                            at: snapshot.saved_at,
                            source: TimeSource::StateChange,
                        },
                    };
                }
                Some(_) => {}
                None => {
                    let updated = chrono::DateTime::parse_from_rfc3339(&pr.updated_at)
                        .ok()
                        .and_then(|time| u64::try_from(time.timestamp()).ok());
                    self.entries.insert(
                        pr.id.clone(),
                        Entry {
                            status,
                            time: ChangeTime {
                                at: updated.unwrap_or(snapshot.saved_at),
                                source: if updated.is_some() {
                                    TimeSource::GitHubUpdate
                                } else {
                                    TimeSource::FirstSeen
                                },
                            },
                        },
                    );
                }
            }
        }
        self.entries.retain(|id, _| present.contains(id));
        self.last_observed_at = snapshot.saved_at;
    }

    pub fn change_time(&self, id: &str) -> Option<ChangeTime> {
        self.entries.get(id).map(|entry| entry.time)
    }
}

pub fn relative_age(at: u64, now: u64) -> String {
    let elapsed = now.saturating_sub(at);
    match elapsed {
        0..60 => "just now".into(),
        60..3600 => format!("{}min ago", elapsed / 60),
        3600..86400 => format!("{}h ago", elapsed / 3600),
        _ => format!("{}d ago", elapsed / 86400),
    }
}
