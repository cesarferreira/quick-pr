# Quick PR

A small native macOS pull request radar. Open it from the menu bar or press **Control–Option–P** to see what needs your review, follow your own PRs, and watch CI.

Built with Rust, GPUI and native AppKit glass. No backend, browser extension, or separate GitHub token setup.

## Use

Install [GitHub CLI](https://cli.github.com/) and sign in:

```sh
brew install gh
gh auth login
```

Then build and launch:

```sh
make install
```

This packages a signed local app and installs it to `~/Applications/Quick PR.app`. Requires macOS 12+, Rust with edition 2024 and file locking support, and Xcode command line tools. GPUI compiles shaders at runtime, so the standalone Metal compiler is not required. Liquid Glass is used on supported macOS versions, with a blurred fallback.

Open at login is on by default. Quick PR then starts when you log in and stays in the menu bar; the panel does not open by itself. Use **Login off** in the panel to stop that. If macOS asks for approval, enable Quick PR in System Settings → General → Login Items. This applies to the installed app. `cargo run` does not add a login item.

- **Relevant:** a combined, deduplicated list of your PRs and PRs requesting your review personally, active in the last **14 days**. Team-only review requests and older activity are excluded. Open PRs you explicitly watch also appear here, regardless of age.
- **Yours:** your open PRs active in the last 14 days, including drafts.
- **Watching:** click **Watch** on any listed PR to follow it. Click **Watching** on the row to unwatch. Hover for a description of either action. Watched PRs remain visible after closing or merging. Maximum 100 watched PRs.
- Click a title to open the PR in your default browser.
- **Pin** keeps the panel visible when you switch apps.
- Filter by title, repository, author or PR number.
- Status badges combine text, icons and color: green for approved/passing, red for changes requested/failing, amber for awaiting review/running, gray for drafts/no checks, and purple for merged PRs. Review and CI states have separate badges.
- Watch, pin, refresh, alerts, open at login, browser and quit controls have hover tooltips.
- Right-click the menu bar icon for Refresh and Quit.
- The menu bar count matches the Relevant list. The activity window appears beside your account name.
- GitHub `updated` activity includes pushes, comments and other changes; it does not measure only your own activity. This relevance rule is explicit and may differ from Dia’s configured Live Group.

| Shortcut | Action |
| --- | --- |
| Control–Option–P | Show / hide |
| Escape | Hide |
| Command–R | Refresh |
| Command–1 / 2 / 3 | Relevant / Yours / Watching |
| Up / Down, Return | Select and open a PR |
| Command–Shift–P | Pin / unpin |

If the global shortcut is already taken, the panel displays a warning and the menu bar still works.

## GitHub access and refresh

Quick PR uses the active **github.com** account in `gh`. It does not read, copy or store your token. Private repositories require the relevant permissions and any organization SSO authorization in GitHub CLI. GitHub Enterprise hosts are not supported in this version.

`gh` still consumes GitHub API quota. Quick PR discovers PR IDs in one GraphQL query, then fetches status in batches of ten to avoid timeouts on large repositories. PRs appearing in multiple tabs are fetched once per refresh. The account lookup is cached by `gh` for one hour. It never polls each PR individually. A refresh for 60 distinct PRs normally uses seven GraphQL requests, rather than a separate request for each PR or check run.

- Refreshes about once a minute while visible and every five minutes while hidden.
- Errors trigger exponential backoff, up to one hour; rate limit errors pause automatic refresh for an hour.
- A nearly exhausted GraphQL budget also pauses automatic refresh for an hour.
- Manual refresh is available, with a ten-second minimum between attempts.
- Network work stays off the UI thread and each `gh` process has a 25-second timeout.
- Each search shows the first 100 PRs, sorted by recent updates. A notice appears when more results exist.

Caches from the previous broad search policy are ignored on upgrade. The last successful snapshot under the current relevance policy remains available offline; its age and any refresh error remain visible. Search indexing can make recent GitHub changes take a little time to appear.

## Alerts

Alerts are off by default. Enable **Alerts on** in the panel for new review requests, failing checks, new discussion and transitions to approved with passing checks. The first sync and unchanged polls are silent. These are status signals, not a guarantee of mergeability or satisfaction of every repository rule.

Notifications currently use macOS AppleScript notifications. Delivery depends on your macOS notification settings for the script notification sender; banners and their app identity may differ from a native notification integration.

## Local data

Preferences and the last PR snapshot live in `~/Library/Application Support/Quick PR/`. This includes PR titles and repository names. The directory and files are restricted to your user. Delete it to reset preferences and the cache.

## Development

```sh
cargo run                  # launch the live app
cargo run -- --demo         # isolated sample data; no GitHub requests or saved preferences
cargo run -- --check        # read-only live GitHub connection check; print counts
cargo test
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
./scripts/package.sh --debug
```

`make package` creates `dist/Quick PR.app`. Packaging uses local ad-hoc signing; distribution notarization is not configured. Nothing creates a repository or pushes code.
