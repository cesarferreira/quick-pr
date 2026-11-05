use crate::native::{self, Event, Shell};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Icon, IconName, Sizable, Theme, ThemeMode,
    input::{Input, InputEvent, InputState},
    tooltip::Tooltip,
};
use quick_pr::github::{self, Preferences, PullRequest, Snapshot};
use std::{
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};

actions!(
    quick_pr,
    [
        Dismiss,
        Refresh,
        TogglePin,
        OpenSelected,
        Next,
        Previous,
        Needs,
        Yours,
        Watching
    ]
);
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", Dismiss, Some("QuickPr")),
        KeyBinding::new("cmd-r", Refresh, Some("QuickPr")),
        KeyBinding::new("cmd-shift-p", TogglePin, Some("QuickPr")),
        KeyBinding::new("enter", OpenSelected, Some("QuickPr")),
        KeyBinding::new("down", Next, Some("QuickPr")),
        KeyBinding::new("up", Previous, Some("QuickPr")),
        KeyBinding::new("cmd-1", Needs, Some("QuickPr")),
        KeyBinding::new("cmd-2", Yours, Some("QuickPr")),
        KeyBinding::new("cmd-3", Watching, Some("QuickPr")),
    ]);
}

type FetchResult = Result<Snapshot, String>;
pub struct QuickPr {
    snapshot: Snapshot,
    prefs: Preferences,
    shell: Shell,
    input: Entity<InputState>,
    focus: FocusHandle,
    tab: usize,
    selected: usize,
    scroll: ScrollHandle,
    pinned: bool,
    loading: bool,
    demo: bool,
    error: Option<String>,
    tx: Sender<FetchResult>,
    rx: Receiver<FetchResult>,
    last_attempt: Instant,
    next_refresh: Instant,
    last_paint: Instant,
    failures: u32,
    login_note: Option<String>,
    _subscriptions: Vec<Subscription>,
}
impl QuickPr {
    pub fn new(shell: Shell, demo: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let snapshot = if demo {
            github::demo()
        } else {
            github::load::<Snapshot>("snapshot.json").current_or_empty()
        };
        let prefs = if demo {
            Preferences::default()
        } else {
            github::load("preferences.json")
        };
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Filter by title, repository or author…")
        });
        let search = cx.subscribe(&input, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.selected = 0;
                this.scroll.scroll_to_item(0);
                cx.notify();
            }
        });
        let appearance = cx.observe_window_appearance(window, |_, window, cx| {
            Theme::change(ThemeMode::from(window.appearance()), Some(window), cx);
            Theme::global_mut(cx).background = transparent_black();
            Theme::global_mut(cx).font_size = px(13.);
            Theme::global_mut(cx).muted_foreground = if Theme::global(cx).mode.is_dark() {
                rgb(0xb6b6c0).into()
            } else {
                rgb(0x60606a).into()
            };
            cx.notify();
        });
        let activation = cx.observe_window_activation(window, |_, window, cx| {
            if !window.is_window_active() {
                cx.spawn_in(window, async move |this, cx| {
                    Timer::after(Duration::from_millis(150)).await;
                    let _ = this.update_in(cx, |this, window, cx| {
                        if !this.pinned
                            && !window.is_window_active()
                            && !native::mouse_button_pressed()
                        {
                            native::hide_panel(window);
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
        });
        let (tx, rx) = mpsc::channel();
        let mut this = Self {
            snapshot,
            prefs,
            shell,
            input,
            focus: cx.focus_handle(),
            tab: 0,
            selected: 0,
            scroll: ScrollHandle::new(),
            pinned: false,
            loading: false,
            demo,
            error: None,
            tx,
            rx,
            last_attempt: Instant::now() - Duration::from_secs(60),
            next_refresh: Instant::now(),
            last_paint: Instant::now(),
            failures: 0,
            login_note: None,
            _subscriptions: vec![search, appearance, activation],
        };
        this.shell.badge(this.snapshot.relevant(&this.prefs).len());
        if !demo {
            this.apply_login_item(false);
            this.refresh(false, cx);
        }
        cx.spawn_in(window, async move |this, cx| {
            loop {
                Timer::after(Duration::from_millis(60)).await;
                if this
                    .update_in(cx, |this, window, cx| this.tick(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        this
    }
    pub fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        native::show_panel(window, cx);
        window.focus(&self.focus);
        let stale = self.snapshot.saved_at == 0
            || github::now().saturating_sub(self.snapshot.saved_at) >= 60;
        if stale
            && (Instant::now() >= self.next_refresh
                || (self.failures == 0 && self.snapshot.remaining >= 50))
        {
            self.refresh(true, cx);
        }
        cx.notify();
    }
    fn apply_login_item(&mut self, interactive: bool) {
        if self.demo {
            return;
        }
        self.login_note = match native::sync_login_item(self.prefs.launch_at_login) {
            native::LoginItemSync::Updated => None,
            native::LoginItemSync::NeedsApproval => Some(
                "macOS needs approval before Quick PR can open at login. Enable it in System Settings → General → Login Items."
                    .into(),
            ),
            native::LoginItemSync::NotInstalledApp if interactive => Some(
                "Open at login is saved. It takes effect from the installed Quick PR app.".into(),
            ),
            native::LoginItemSync::Unsupported if interactive => {
                Some("Open at login needs macOS 13 or later.".into())
            }
            native::LoginItemSync::Failed(message) => Some(message),
            native::LoginItemSync::NotInstalledApp | native::LoginItemSync::Unsupported => None,
        };
    }
    fn refresh(&mut self, manual: bool, cx: &mut Context<Self>) {
        if self.demo || self.loading || self.last_attempt.elapsed() < Duration::from_secs(10) {
            return;
        }
        if !manual && Instant::now() < self.next_refresh {
            return;
        }
        self.loading = true;
        self.last_attempt = Instant::now();
        let prefs = self.prefs.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(github::fetch(&prefs).map_err(|e| format!("{e:#}")));
        });
        cx.notify();
    }
    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for event in self.shell.poll() {
            match event {
                Event::Toggle => {
                    if native::panel_visible(window) {
                        native::hide_panel(window);
                    } else {
                        self.shell.show_at_menu(window, cx);
                        window.focus(&self.focus);
                        self.show(window, cx);
                    }
                }
                Event::Refresh => self.refresh(true, cx),
                Event::Quit => cx.quit(),
            }
        }
        while let Ok(result) = self.rx.try_recv() {
            self.loading = false;
            match result {
                Ok(snapshot) => {
                    if self.prefs.notifications {
                        let messages = github::changes(&self.snapshot, &snapshot);
                        if !messages.is_empty() {
                            native::notify(
                                messages.into_iter().take(3).collect::<Vec<_>>().join("\n"),
                            );
                        }
                    }
                    self.snapshot = snapshot;
                    self.failures = 0;
                    self.error = None;
                    if let Err(e) = github::save("snapshot.json", &self.snapshot) {
                        self.error = Some(format!("Could not save offline cache: {e}"));
                    }
                    self.shell.badge(self.snapshot.relevant(&self.prefs).len());
                }
                Err(error) => {
                    self.failures = self.failures.saturating_add(1);
                    if error.to_lowercase().contains("rate limit") || error.contains("429") {
                        self.failures = 6;
                    }
                    self.error = Some(error);
                }
            }
            let delay = if self.snapshot.remaining < 50 && self.snapshot.saved_at != 0 {
                3600
            } else {
                github::refresh_delay(native::panel_visible(window), self.failures)
            };
            self.next_refresh = Instant::now() + Duration::from_secs(delay);
            cx.notify();
        }
        // Recompute freshness from the last attempt so closing the panel slows polling.
        let delay = github::refresh_delay(native::panel_visible(window), self.failures);
        if self.last_attempt.elapsed() >= Duration::from_secs(delay)
            && Instant::now() >= self.next_refresh
        {
            self.refresh(false, cx);
        }
        if native::panel_visible(window) && self.last_paint.elapsed() >= Duration::from_secs(1) {
            self.last_paint = Instant::now();
            cx.notify();
        }
    }
    fn rows(&self, cx: &App) -> Vec<PullRequest> {
        let watching;
        let relevant;
        let list = match self.tab {
            0 => {
                relevant = self.snapshot.relevant(&self.prefs);
                &relevant
            }
            1 => &self.snapshot.yours,
            _ => {
                let mut all = self.snapshot.watching.clone();
                for pr in self.snapshot.needs.iter().chain(&self.snapshot.yours) {
                    if !all.iter().any(|p| p.id == pr.id) {
                        all.push(pr.clone());
                    }
                }
                watching = all
                    .into_iter()
                    .filter(|p| self.prefs.watched.contains(&p.id))
                    .collect::<Vec<_>>();
                &watching
            }
        };
        let filter = self.input.read(cx).value().to_lowercase();
        list.iter()
            .filter(|p| {
                format!("{} {} {} #{}", p.title, p.repo, p.author, p.number)
                    .to_lowercase()
                    .contains(&filter)
            })
            .cloned()
            .collect()
    }
    fn watch(&mut self, pr: PullRequest, cx: &mut Context<Self>) {
        if self.prefs.watched.contains(&pr.id) {
            self.prefs.watched.retain(|id| id != &pr.id);
        } else if self.prefs.watched.len() < 100 {
            self.prefs.watched.push(pr.id.clone());
            if !self.snapshot.watching.iter().any(|p| p.id == pr.id) {
                self.snapshot.watching.push(pr);
            }
        } else {
            self.error = Some("You can watch up to 100 PRs. Unwatch one to add another.".into());
        }
        self.shell.badge(self.snapshot.relevant(&self.prefs).len());
        if !self.demo {
            if let Err(e) = github::save("preferences.json", &self.prefs) {
                self.error = Some(format!("Could not save watched PRs: {e}"));
            }
            if let Err(e) = github::save("snapshot.json", &self.snapshot) {
                self.error = Some(format!("Could not save offline cache: {e}"));
            }
        }
        cx.notify();
    }
    fn select_tab(&mut self, tab: usize, cx: &mut Context<Self>) {
        self.tab = tab;
        self.selected = 0;
        self.scroll.scroll_to_item(0);
        cx.notify();
    }
    fn open_url(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(e) = open::that(url) {
            self.error = Some(format!("Could not open browser: {e}"));
            cx.notify();
        } else if !self.pinned {
            native::hide_panel(window);
        }
    }
    fn button(&self, id: &'static str, text: &str) -> Stateful<Div> {
        let hint = match id {
            "pin" if text == "Pin" => "Keep this panel visible when switching apps · ⌘⇧P",
            "pin" => "Hide this panel when switching apps · ⌘⇧P",
            "refresh" => "Refresh pull request status · ⌘R",
            "notifications" if text == "Alerts off" => "Enable notifications for PR status changes",
            "notifications" => "Disable notifications for PR status changes",
            "login" if text == "Login off" => "Open Quick PR when you log in",
            "login" => "Stop opening Quick PR when you log in",
            "github" => "Open your pull requests on GitHub",
            "quit" => "Quit Quick PR",
            _ => text,
        }
        .to_owned();
        div()
            .id(id)
            .tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx))
            .px_2()
            .py_1()
            .rounded_lg()
            .cursor_pointer()
            .text_size(px(11.))
            .child(text.to_string())
            .hover(|s| s.bg(rgba(0x88888822)))
    }
    fn pr_row(&self, pr: PullRequest, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let selected = index == self.selected;
        let watched = self.prefs.watched.contains(&pr.id);
        let dark = cx.theme().mode.is_dark();
        let green = if dark { 0x54cba0 } else { 0x16734e };
        let red = if dark { 0xef7777 } else { 0xb93838 };
        let amber = if dark { 0xdab36a } else { 0x895b0a };
        let purple = if dark { 0xa78bfa } else { 0x7852b8 };
        let gray = if dark { 0xb6b6c0 } else { 0x60606a };
        let (review_icon, review_color) = match pr.state.as_str() {
            "MERGED" => (IconName::CircleCheck, purple),
            "CLOSED" => (IconName::CircleX, gray),
            _ if pr.draft => (IconName::File, gray),
            _ => match pr.review.as_str() {
                "APPROVED" => (IconName::CircleCheck, green),
                "CHANGES_REQUESTED" => (IconName::TriangleAlert, red),
                _ => (IconName::CircleUser, amber),
            },
        };
        let (checks_icon, checks_color) = match pr.checks.as_str() {
            "SUCCESS" => (IconName::Check, green),
            "FAILURE" | "ERROR" => (IconName::CircleX, red),
            "PENDING" | "EXPECTED" => (IconName::LoaderCircle, amber),
            _ => (IconName::Dash, gray),
        };
        let badge = |text: &str, icon: IconName, color: u32| {
            div()
                .flex()
                .items_center()
                .gap_1()
                .px(px(6.))
                .py(px(2.))
                .rounded_md()
                .bg(rgba((color << 8) | 0x18))
                .text_color(rgb(color))
                .child(Icon::new(icon).size_3())
                .child(text.to_owned())
        };
        let color = if pr.state == "MERGED" {
            purple
        } else if checks_color == red || review_color == red {
            red
        } else {
            review_color
        };
        let url = pr.url.clone();
        let star = pr.clone();
        div()
            .id(SharedString::from(format!("row-{}", pr.id)))
            .flex()
            .items_start()
            .gap_2()
            .px_2()
            .py_2()
            .rounded_xl()
            .w_full()
            .when(selected, |s| s.bg(rgba(0x8888881c)))
            .hover(|s| s.bg(rgba(0x88888827)))
            .child(
                div()
                    .pt_1()
                    .text_color(rgb(color))
                    .child(Icon::new(IconName::GitHub).size(px(14.))),
            )
            .child(
                div()
                    .id(SharedString::from(format!("open-{}", pr.id)))
                    .flex_1()
                    .min_w_0()
                    .cursor_pointer()
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.open_url(&url, window, cx)),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(pr.title.clone()),
                    )
                    .child(
                        div()
                            .mt(px(2.))
                            .text_size(px(11.))
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{}  ·  #{}  ·  {}", pr.repo, pr.number, pr.author)),
                    )
                    .child(
                        div()
                            .mt_1()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_size(px(11.))
                            .flex_wrap()
                            .child(badge(pr.review_label(), review_icon, review_color))
                            .child(badge(pr.checks_label(), checks_icon, checks_color)),
                    ),
            )
            .child(
                div()
                    .id(SharedString::from(format!("watch-{}", pr.id)))
                    .flex()
                    .items_center()
                    .gap_1()
                    .px(px(4.))
                    .py(px(2.))
                    .text_size(px(11.))
                    .flex_shrink_0()
                    .tooltip(move |window, cx| {
                        Tooltip::new(if watched {
                            "Stop watching this PR"
                        } else {
                            "Watch this PR — keep it in the Watching tab"
                        })
                        .build(window, cx)
                    })
                    .rounded_md()
                    .cursor_pointer()
                    .text_color(if watched {
                        rgb(0xdab36a).into()
                    } else {
                        cx.theme().muted_foreground
                    })
                    .hover(|s| s.bg(rgba(0x88888833)))
                    .on_click(cx.listener(move |this, _, _, cx| this.watch(star.clone(), cx)))
                    .child(Icon::new(IconName::Star).size(px(14.)))
                    .child(if watched { "Watching" } else { "Watch" }),
            )
            .into_any_element()
    }
}
impl Render for QuickPr {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.rows(cx);
        self.selected = self.selected.min(rows.len().saturating_sub(1));
        let muted = cx.theme().muted_foreground;
        let dark = cx.theme().mode.is_dark();
        let age = github::now().saturating_sub(self.snapshot.saved_at);
        let updated = if self.demo {
            "Demo preview".into()
        } else if self.loading {
            "Refreshing…".into()
        } else if self.snapshot.saved_at == 0 {
            "Not connected".into()
        } else if age < 60 {
            format!("Updated {age}s ago")
        } else {
            format!("Updated {}m ago", age / 60)
        };
        let mut body = div()
            .key_context("QuickPr")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .font_family(".SystemUIFont")
            .text_size(px(13.))
            .text_color(cx.theme().foreground)
            .bg(if dark {
                rgba(0x17171c80)
            } else {
                rgba(0xffffff9a)
            })
            .on_action(cx.listener(|_, _: &Dismiss, window, _| native::hide_panel(window)))
            .on_action(cx.listener(|this, _: &Refresh, _, cx| this.refresh(true, cx)))
            .on_action(cx.listener(|this, _: &TogglePin, _, cx| {
                this.pinned = !this.pinned;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Needs, _, cx| this.select_tab(0, cx)))
            .on_action(cx.listener(|this, _: &Yours, _, cx| this.select_tab(1, cx)))
            .on_action(cx.listener(|this, _: &Watching, _, cx| this.select_tab(2, cx)))
            .on_action(cx.listener(|this, _: &Next, _, cx| {
                this.selected = (this.selected + 1).min(this.rows(cx).len().saturating_sub(1));
                this.scroll.scroll_to_item(this.selected);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Previous, _, cx| {
                this.selected = this.selected.saturating_sub(1);
                this.scroll.scroll_to_item(this.selected);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &OpenSelected, window, cx| {
                if let Some(p) = this.rows(cx).get(this.selected) {
                    this.open_url(&p.url, window, cx);
                }
            }))
            .child(
                div()
                    .flex_shrink_0()
                    .px_3()
                    .pt_3()
                    .pb_3()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(Icon::new(IconName::GitHub).size_4())
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .on_mouse_down(MouseButton::Left, |_, window, _| {
                                        native::start_drag(window)
                                    })
                                    .child("Pull Requests"),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                self.button("pin", if self.pinned { "Unpin" } else { "Pin" })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.pinned = !this.pinned;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                self.button("refresh", "↻")
                                    .on_click(cx.listener(|this, _, _, cx| this.refresh(true, cx))),
                            ),
                    ),
            )
            .child(
                div()
                    .px_3()
                    .pb_2()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_size(px(11.))
                    .text_color(muted)
                    .child(if self.snapshot.login.is_empty() {
                        "Your pull requests, at a glance".into()
                    } else {
                        format!(
                            "@{} · Last {} days",
                            self.snapshot.login,
                            github::RELEVANCE_DAYS
                        )
                    })
                    .child(updated),
            );
        let counts = [
            self.snapshot.relevant(&self.prefs).len(),
            self.snapshot.yours.len(),
            self.prefs.watched.len(),
        ];
        let mut tabs = div()
            .mx_3()
            .p_1()
            .flex()
            .gap_1()
            .rounded_lg()
            .bg(rgba(0x88888818));
        for (i, name) in ["Relevant", "Yours", "Watching"].iter().enumerate() {
            tabs = tabs.child(
                div()
                    .id(SharedString::from(format!("tab-{i}")))
                    .flex_1()
                    .text_center()
                    .py_1()
                    .rounded_md()
                    .text_size(px(12.))
                    .cursor_pointer()
                    .text_color(if self.tab == i {
                        cx.theme().foreground
                    } else {
                        muted
                    })
                    .when(self.tab == i, |s| {
                        s.bg(if dark {
                            rgba(0xffffff15)
                        } else {
                            rgba(0xffffffbb)
                        })
                        .font_weight(FontWeight::MEDIUM)
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.select_tab(i, cx)))
                    .child(format!("{name}  {}", counts[i])),
            );
        }
        body = body.child(tabs).child(
            div().px_3().pt_2().pb_1().child(
                Input::new(&self.input)
                    .xsmall()
                    .bordered(false)
                    .cleanable(true),
            ),
        );
        if let Some(note) = &self.login_note {
            body = body.child(
                div()
                    .mx_3()
                    .mb_2()
                    .text_size(px(11.))
                    .text_color(rgb(0xdab36a))
                    .child(note.clone()),
            );
        }
        if let Some(warning) = &self.shell.warning {
            body = body.child(
                div()
                    .mx_3()
                    .mb_2()
                    .text_size(px(11.))
                    .text_color(rgb(0xdab36a))
                    .child(warning.clone()),
            );
        }
        if let Some(error) = &self.error {
            body = body.child(
                div()
                    .mx_3()
                    .my_2()
                    .p_3()
                    .rounded_lg()
                    .bg(rgba(0xef777718))
                    .text_size(px(11.))
                    .text_color(rgb(if dark { 0xffaaaa } else { 0x9b3030 }))
                    .child(error.clone()),
            );
        }
        let mut list = div()
            .id("pr-list")
            .track_scroll(&self.scroll)
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_3()
            .py_1();
        if rows.is_empty() {
            let (title, detail) = if self.loading && self.snapshot.saved_at == 0 {
                ("Connecting to GitHub", "Using your existing gh login…")
            } else if self.snapshot.saved_at == 0 {
                (
                    "Connect GitHub CLI",
                    "Run brew install gh, then gh auth login in Terminal. Hit Refresh when ready.",
                )
            } else if !self.input.read(cx).value().is_empty() {
                (
                    "No matching pull requests",
                    "Try another title, repository or author.",
                )
            } else if self.tab == 0 {
                (
                    "All caught up",
                    "No recently active PRs of yours or personal review requests.",
                )
            } else if self.tab == 1 {
                (
                    "A clean slate",
                    "Your PRs active in the last 14 days will appear here.",
                )
            } else {
                (
                    "Keep an eye on a PR",
                    "Click Watch beside a PR to follow it. Watched PRs stay here after merging or closing.",
                )
            };
            list = list.child(
                div()
                    .px_4()
                    .py_6()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_3()
                    .text_center()
                    .text_color(muted)
                    .child(Icon::new(IconName::Inbox).size_6())
                    .child(div().text_size(px(13.)).child(title))
                    .child(div().text_size(px(12.)).child(detail)),
            );
        } else {
            for (i, pr) in rows.into_iter().enumerate() {
                list = list.child(self.pr_row(pr, i, cx));
            }
        }
        body = body.child(list);
        if self.snapshot.truncated {
            body =
                body.child(
                    div().px_3().text_size(px(11.)).text_color(muted).child(
                        "Showing the first 100 PRs per search. Open GitHub for the full list.",
                    ),
                );
        }
        body.child(
            div()
                .flex_shrink_0()
                .mt_1()
                .px_2()
                .py_1()
                .border_t_1()
                .border_color(rgba(0x88888822))
                .flex()
                .items_center()
                .justify_between()
                .text_size(px(11.))
                .text_color(muted)
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_1()
                        .child(
                            self.button(
                                "notifications",
                                if self.prefs.notifications {
                                    "Alerts on"
                                } else {
                                    "Alerts off"
                                },
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.prefs.notifications = !this.prefs.notifications;
                                if !this.demo
                                    && let Err(e) = github::save("preferences.json", &this.prefs)
                                {
                                    this.error = Some(e.to_string());
                                }
                                cx.notify();
                            })),
                        )
                        .child(
                            self.button(
                                "login",
                                if self.prefs.launch_at_login {
                                    "Login on"
                                } else {
                                    "Login off"
                                },
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.prefs.launch_at_login = !this.prefs.launch_at_login;
                                if !this.demo {
                                    if let Err(e) = github::save("preferences.json", &this.prefs) {
                                        this.prefs.launch_at_login = !this.prefs.launch_at_login;
                                        this.error =
                                            Some(format!("Could not save open at login: {e}"));
                                    } else {
                                        this.apply_login_item(true);
                                    }
                                }
                                cx.notify();
                            })),
                        )
                        .child(self.button("github", "Open GitHub ↗").on_click(cx.listener(
                            |this, _, window, cx| {
                                this.open_url("https://github.com/pulls", window, cx)
                            },
                        ))),
                )
                .child(self.button("quit", "Quit").on_click(|_, _, cx| cx.quit())),
        )
    }
}
