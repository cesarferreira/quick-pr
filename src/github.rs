use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PullRequest {
    pub id: String,
    pub title: String,
    pub url: String,
    pub number: u64,
    pub repo: String,
    pub author: String,
    pub draft: bool,
    pub state: String,
    pub review: String,
    pub checks: String,
    pub comments: u64,
}
impl PullRequest {
    pub fn from_node(v: &Value) -> Option<Self> {
        if v["__typename"] != "PullRequest" {
            return None;
        }
        let url = v["url"].as_str()?.to_string();
        if !url.starts_with("https://github.com/") {
            return None;
        }
        Some(Self {
            id: v["id"].as_str()?.into(),
            title: v["title"].as_str()?.into(),
            url,
            number: v["number"].as_u64()?,
            repo: v["repository"]["nameWithOwner"].as_str()?.into(),
            author: v["author"]["login"]
                .as_str()
                .unwrap_or("deleted user")
                .into(),
            draft: v["isDraft"].as_bool().unwrap_or(false),
            state: v["state"].as_str().unwrap_or("OPEN").into(),
            review: v["reviewDecision"].as_str().unwrap_or("").into(),
            checks: v["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["state"]
                .as_str()
                .unwrap_or("")
                .into(),
            comments: ["comments", "reviews", "reviewThreads"]
                .iter()
                .map(|k| v[k]["totalCount"].as_u64().unwrap_or(0))
                .sum(),
        })
    }
    pub fn review_label(&self) -> &str {
        match self.state.as_str() {
            "MERGED" => "Merged",
            "CLOSED" => "Closed",
            _ if self.draft => "Draft",
            _ => match self.review.as_str() {
                "APPROVED" => "Approved",
                "CHANGES_REQUESTED" => "Changes requested",
                _ => "Awaiting review",
            },
        }
    }
    pub fn checks_label(&self) -> &str {
        match self.checks.as_str() {
            "SUCCESS" => "Checks passed",
            "FAILURE" | "ERROR" => "Checks failed",
            "PENDING" | "EXPECTED" => "Checks running",
            _ => "No checks",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    #[serde(default)]
    pub relevance_version: u32,
    pub login: String,
    pub needs: Vec<PullRequest>,
    pub yours: Vec<PullRequest>,
    pub watching: Vec<PullRequest>,
    pub saved_at: u64,
    pub remaining: u64,
    pub reset_at: String,
    pub truncated: bool,
}
const RELEVANCE_VERSION: u32 = 1;
pub const RELEVANCE_DAYS: u64 = 14;

impl Snapshot {
    pub fn relevant(&self, prefs: &Preferences) -> Vec<PullRequest> {
        let mut result = Vec::new();
        for pr in self.needs.iter().chain(&self.yours).chain(
            self.watching
                .iter()
                .filter(|pr| prefs.watched.contains(&pr.id)),
        ) {
            if pr.state == "OPEN" && !result.iter().any(|p: &PullRequest| p.id == pr.id) {
                result.push(pr.clone());
            }
        }
        result
    }
    pub fn current_or_empty(self) -> Self {
        if self.relevance_version == RELEVANCE_VERSION {
            self
        } else {
            Self::default()
        }
    }
}

pub fn relevance_queries(login: &str, at: u64) -> (String, String) {
    let cutoff = at.saturating_sub(RELEVANCE_DAYS * 86400);
    let date = chrono::DateTime::from_timestamp(cutoff as i64, 0)
        .unwrap_or_default()
        .format("%Y-%m-%d");
    let recent = format!("is:pr is:open updated:>={date}");
    (
        format!("{recent} author:{login} sort:updated-desc"),
        format!("{recent} user-review-requested:{login} sort:updated-desc"),
    )
}

fn default_launch_at_login() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(default)]
    pub watched: Vec<String>,
    #[serde(default)]
    pub notifications: bool,
    #[serde(default = "default_launch_at_login")]
    pub launch_at_login: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            watched: Vec::new(),
            notifications: false,
            launch_at_login: default_launch_at_login(),
        }
    }
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn refresh_delay(visible: bool, failures: u32) -> u64 {
    let base: u64 = if visible { 60 } else { 300 };
    base.saturating_mul(1u64 << failures.min(6)).min(3600)
}
pub fn parse_response(raw: &str) -> Result<Value> {
    let response: Value = serde_json::from_str(raw).context("GitHub returned invalid JSON")?;
    if let Some(errors) = response["errors"].as_array().filter(|e| !e.is_empty()) {
        bail!(
            "{}",
            errors
                .iter()
                .filter_map(|e| e["message"].as_str())
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
    response
        .get("data")
        .filter(|v| !v.is_null())
        .cloned()
        .context("GitHub returned no data")
}

fn gh_path() -> Result<PathBuf> {
    for path in std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .chain([
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ])
    {
        let exe = path.join("gh");
        if exe.is_file() {
            return Ok(exe);
        }
    }
    bail!(
        "GitHub CLI is missing. Install it with `brew install gh`, then run `gh auth login` in Terminal."
    )
}

pub fn run_gh(args: &[&str]) -> Result<String> {
    let mut child = Command::new(gh_path()?)
        .args(args)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_HOST", "github.com")
        .env("GH_PAGER", "cat")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Could not start gh")?;
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let out = std::thread::spawn(move || {
        let mut s = String::new();
        stdout.read_to_string(&mut s).map(|_| s)
    });
    let err = std::thread::spawn(move || {
        let mut s = String::new();
        stderr.read_to_string(&mut s).map(|_| s)
    });
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() > Duration::from_secs(25) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = out.join();
            let _ = err.join();
            bail!("GitHub took too long to respond. Your last saved PRs are still available.");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let stdout = out
        .join()
        .map_err(|_| anyhow::anyhow!("gh output reader failed"))??;
    let stderr = err
        .join()
        .map_err(|_| anyhow::anyhow!("gh error reader failed"))??;
    if !status.success() {
        // Tokens never enter this process: gh owns authentication and keychain access.
        bail!(
            "{}",
            if stderr.trim().is_empty() {
                "GitHub request failed. Check `gh auth status` in Terminal."
            } else {
                stderr.trim()
            }
        );
    }
    Ok(stdout)
}

const FIELDS: &str = r#"
    __typename id title url number state isDraft reviewDecision
    author { login } repository { nameWithOwner }
    comments { totalCount } reviews { totalCount } reviewThreads { totalCount }
    commits(last: 1) { nodes { commit { statusCheckRollup { state } } } }
"#;

pub fn fetch(prefs: &Preferences) -> Result<Snapshot> {
    let started = Instant::now();
    let login = run_gh(&[
        "api",
        "--hostname",
        "github.com",
        "user",
        "--cache",
        "1h",
        "--jq",
        ".login",
    ])?;
    let login = login.trim();
    anyhow::ensure!(
        !login.is_empty() && login.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
        "Invalid GitHub login"
    );
    let discovery = r#"query($mine: String!, $needs: String!) {
        viewer { login }
        mine: search(query: $mine, type: ISSUE, first: 100) { pageInfo { hasNextPage } nodes { ... on PullRequest { id } } }
        needs: search(query: $needs, type: ISSUE, first: 100) { pageInfo { hasNextPage } nodes { ... on PullRequest { id } } }
        rateLimit { remaining resetAt }
    }"#;
    let (mine, needs) = relevance_queries(login, now());
    let mine_query = format!("mine={mine}");
    let needs_query = format!("needs={needs}");
    let discovery_query = format!("query={discovery}");
    let data = parse_response(&run_gh(&[
        "api",
        "--hostname",
        "github.com",
        "graphql",
        "-f",
        &discovery_query,
        "-f",
        &mine_query,
        "-f",
        &needs_query,
    ])?)?;
    let ids = |key: &str| -> Vec<String> {
        data[key]["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|n| n["id"].as_str().map(str::to_owned))
            .collect()
    };
    let mine = ids("mine");
    let needs = ids("needs");
    let mut all = Vec::new();
    for id in mine
        .iter()
        .chain(&needs)
        .chain(prefs.watched.iter().take(100))
    {
        if !all.contains(id) {
            all.push(id.clone());
        }
    }
    let mut prs = Vec::new();
    let mut remaining = data["rateLimit"]["remaining"].as_u64().unwrap_or(0);
    let mut reset_at = data["rateLimit"]["resetAt"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let query = format!(
        "query($ids: [ID!]!) {{ nodes(ids: $ids) {{ ... on PullRequest {{ {FIELDS} }} }} rateLimit {{ remaining resetAt }} }}"
    );
    // Bounded detail batches avoid GitHub timeouts on large repositories. A PR that
    // appears in several tabs is fetched only once. There are no per-PR requests.
    for batch in all.chunks(10) {
        anyhow::ensure!(
            started.elapsed() < Duration::from_secs(90),
            "GitHub refresh took too long. Keeping the previous snapshot."
        );
        anyhow::ensure!(
            remaining >= 50,
            "GitHub rate limit budget is low; automatic refresh will back off."
        );
        let mut args = vec![
            "api".to_owned(),
            "--hostname".into(),
            "github.com".into(),
            "graphql".into(),
            "-f".into(),
            format!("query={query}"),
        ];
        for id in batch {
            args.extend(["-f".into(), format!("ids[]={id}")]);
        }
        let detail = parse_response(&run_gh(
            &args.iter().map(String::as_str).collect::<Vec<_>>(),
        )?)?;
        prs.extend(
            detail["nodes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(PullRequest::from_node),
        );
        remaining = detail["rateLimit"]["remaining"].as_u64().unwrap_or(0);
        reset_at = detail["rateLimit"]["resetAt"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
    }
    let select = |ids: &[String]| {
        ids.iter()
            .filter_map(|id| prs.iter().find(|p| &p.id == id).cloned())
            .collect()
    };
    Ok(Snapshot {
        relevance_version: RELEVANCE_VERSION,
        login: data["viewer"]["login"].as_str().unwrap_or(login).into(),
        needs: select(&needs),
        yours: select(&mine),
        watching: select(&prefs.watched),
        saved_at: now(),
        remaining,
        reset_at,
        truncated: data["mine"]["pageInfo"]["hasNextPage"] == true
            || data["needs"]["pageInfo"]["hasNextPage"] == true,
    })
}

pub fn data_dir() -> Result<PathBuf> {
    let path = dirs::data_local_dir()
        .context("Could not find Application Support")?
        .join("Quick PR");
    std::fs::create_dir_all(&path)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
    Ok(path)
}
pub fn load<T: serde::de::DeserializeOwned + Default>(name: &str) -> T {
    data_dir()
        .ok()
        .and_then(|p| std::fs::read(p.join(name)).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}
pub fn save<T: Serialize>(name: &str, value: &T) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let path = data_dir()?.join(name);
    let tmp = path.with_extension("tmp");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

pub fn changes(old: &Snapshot, new: &Snapshot) -> Vec<String> {
    if old.saved_at == 0 || old.login != new.login {
        return vec![];
    }
    let mut messages = vec![];
    for pr in &new.needs {
        if !old.needs.iter().any(|p| p.id == pr.id) {
            messages.push(format!("Review requested: {}", pr.title));
        }
    }
    for pr in new.yours.iter().chain(&new.watching) {
        let previous = old
            .yours
            .iter()
            .chain(&old.watching)
            .find(|p| p.id == pr.id);
        if let Some(prev) = previous {
            if pr.checks != prev.checks && ["FAILURE", "ERROR"].contains(&pr.checks.as_str()) {
                messages.push(format!("Checks failed: {}", pr.title));
            } else if pr.review == "APPROVED"
                && pr.checks == "SUCCESS"
                && !pr.draft
                && pr.state == "OPEN"
                && (prev.review != "APPROVED" || prev.checks != "SUCCESS")
            {
                messages.push(format!("Ready to merge: {}", pr.title));
            } else if pr.comments > prev.comments {
                messages.push(format!("New discussion: {}", pr.title));
            }
        }
    }
    messages.sort();
    messages.dedup();
    messages
}

pub fn demo() -> Snapshot {
    let pr = |id: &str, title: &str, repo: &str, review: &str, checks: &str, draft| PullRequest {
        id: id.into(),
        title: title.into(),
        repo: repo.into(),
        author: "demo-user".into(),
        number: 1842,
        review: review.into(),
        checks: checks.into(),
        draft,
        state: "OPEN".into(),
        url: "https://github.com/pulls".into(),
        comments: 2,
    };
    Snapshot {
        relevance_version: RELEVANCE_VERSION,
        login: "demo-user".into(),
        saved_at: now(),
        remaining: 4980,
        needs: vec![
            pr(
                "a",
                "Improve keyboard navigation",
                "demo/taskboard",
                "REVIEW_REQUIRED",
                "SUCCESS",
                false,
            ),
            pr(
                "b",
                "Add a compact settings panel",
                "demo/taskboard",
                "REVIEW_REQUIRED",
                "PENDING",
                false,
            ),
        ],
        yours: vec![
            pr(
                "c",
                "Add dark mode preferences",
                "demo/taskboard",
                "",
                "PENDING",
                true,
            ),
            pr(
                "d",
                "Polish the search experience",
                "demo/ui-kit",
                "APPROVED",
                "SUCCESS",
                false,
            ),
            pr(
                "e",
                "Fix session restore after sleep",
                "demo/taskboard",
                "CHANGES_REQUESTED",
                "FAILURE",
                false,
            ),
        ],
        ..Default::default()
    }
}
