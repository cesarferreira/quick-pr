use crate::github::PullRequest;
use std::collections::BTreeSet;

pub struct Group {
    pub prs: Vec<PullRequest>,
}
impl Group {
    pub fn is_stack(&self) -> bool {
        self.prs.len() > 1
    }
    pub fn key(&self) -> &str {
        &self.prs[0].id
    }
    pub fn shared_author(&self) -> bool {
        self.prs
            .iter()
            .all(|pr| pr.author.eq_ignore_ascii_case(&self.prs[0].author))
    }
    pub fn matches(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        self.prs.iter().any(|pr| {
            format!("{} {} @{} #{}", pr.title, pr.repo, pr.author, pr.number)
                .to_lowercase()
                .contains(&query)
        })
    }
}

/// Only draw unambiguous linear chains among PRs already present in this view.
/// Match both repository identity and case-sensitive branch names, never titles.
pub fn groups(prs: &[PullRequest]) -> Vec<Group> {
    let mut parents = vec![None; prs.len()];
    for (child, pr) in prs.iter().enumerate() {
        if pr.base_ref.is_empty() || pr.base_repo.is_empty() {
            continue;
        }
        let candidates: Vec<_> = prs
            .iter()
            .enumerate()
            .filter(|(index, parent)| {
                *index != child
                    && !parent.head_ref.is_empty()
                    && !parent.head_repo.is_empty()
                    && parent.repo.eq_ignore_ascii_case(&pr.repo)
                    && parent.head_repo.eq_ignore_ascii_case(&pr.base_repo)
                    && parent.head_ref == pr.base_ref
            })
            .map(|(index, _)| index)
            .collect();
        if candidates.len() == 1 {
            parents[child] = Some(candidates[0]);
        }
    }
    let mut children = vec![Vec::new(); prs.len()];
    for (child, parent) in parents.iter().enumerate() {
        if let Some(parent) = parent {
            children[*parent].push(child);
        }
    }
    let mut chains = Vec::new();
    let mut membership = vec![None; prs.len()];
    for (root, parent) in parents.iter().enumerate() {
        if parent.is_some() {
            continue;
        }
        let mut chain = vec![root];
        let mut current = root;
        while children[current].len() == 1 {
            current = children[current][0];
            if chain.contains(&current) {
                break;
            }
            chain.push(current);
        }
        // A branching component cannot be honestly labeled Base → Top.
        if chain.len() > 1 && children[current].is_empty() {
            for index in &chain {
                membership[*index] = Some(chains.len());
            }
            chains.push(chain);
        }
    }
    let mut emitted = BTreeSet::new();
    let mut result = Vec::new();
    for (index, pr) in prs.iter().enumerate() {
        match membership[index] {
            Some(chain) if emitted.insert(chain) => result.push(Group {
                prs: chains[chain]
                    .iter()
                    .map(|index| prs[*index].clone())
                    .collect(),
            }),
            None => result.push(Group {
                prs: vec![pr.clone()],
            }),
            _ => {}
        }
    }
    result
}

pub struct VisibleRow {
    pub pr: PullRequest,
    pub scroll_index: usize,
    pub collapsed: bool,
}

/// Group headers occupy scroll elements, but only collapsed headers occupy a
/// keyboard position. Enter on a collapsed header expands its stack.
pub fn navigation(groups: &[Group], collapsed: &BTreeSet<String>) -> Vec<VisibleRow> {
    let mut result = Vec::new();
    let mut scroll_index = 0;
    for group in groups {
        if group.is_stack() && collapsed.contains(group.key()) {
            result.push(VisibleRow {
                pr: group.prs[0].clone(),
                scroll_index,
                collapsed: true,
            });
            scroll_index += 1;
        } else {
            if group.is_stack() {
                scroll_index += 1;
            }
            for pr in &group.prs {
                result.push(VisibleRow {
                    pr: pr.clone(),
                    scroll_index,
                    collapsed: false,
                });
                scroll_index += 1;
            }
        }
    }
    result
}
