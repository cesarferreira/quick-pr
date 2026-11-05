mod native;
mod ui;
use gpui::*;
use gpui_component::{Root, Theme, ThemeMode};
use quick_pr::github;
use std::fs::OpenOptions;

struct Panel(Entity<ui::QuickPr>);
impl Global for Panel {}
struct Lock {
    _file: std::fs::File,
}
impl Global for Lock {}

fn main() -> anyhow::Result<()> {
    let demo = std::env::args().any(|a| a == "--demo");
    if std::env::args().any(|a| a == "--check") {
        let prefs = github::load("preferences.json");
        let snapshot = github::fetch(&prefs)?;
        println!(
            "Connected as {}: {} relevant PRs, {} direct review requests, {} authored, {} watched. GraphQL budget remaining: {}.{}",
            snapshot.login,
            snapshot.relevant(&prefs).len(),
            snapshot.needs.len(),
            snapshot.yours.len(),
            snapshot.watching.len(),
            snapshot.remaining,
            if snapshot.truncated {
                " Showing first 100 per search."
            } else {
                ""
            }
        );
        return Ok(());
    }
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(github::data_dir()?.join(if demo { "demo.lock" } else { "app.lock" }))?;
    if lock.try_lock().is_err() {
        anyhow::bail!("Quick PR is already running. Click its menu bar icon or press ⌃⌥P.");
    }
    let app = Application::new().with_assets(gpui_component_assets::Assets);
    app.on_reopen(|cx| {
        if let Some(window) = cx.windows().first().copied() {
            let view = cx.try_global::<Panel>().map(|p| p.0.clone());
            let _ = window.update(cx, |_, window, cx| {
                if let Some(view) = view {
                    view.update(cx, |v, cx| v.show(window, cx));
                }
            });
        }
    });
    app.run(move |cx| {
        cx.set_global(Lock { _file: lock });
        native::prepare_app();
        gpui_component::init(cx);
        ui::bind_keys(cx);
        let shell = match native::Shell::new() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{e:#}");
                cx.quit();
                return;
            }
        };
        let background = native::launched_at_login();
        let bounds = Bounds::centered(None, size(px(480.), px(460.)), cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: None,
                kind: WindowKind::PopUp,
                show: false,
                focus: false,
                is_resizable: true,
                is_minimizable: false,
                window_min_size: Some(size(px(420.), px(300.))),
                window_background: WindowBackgroundAppearance::Transparent,
                app_id: Some("com.cesarferreira.quick-pr".into()),
                ..Default::default()
            },
            |window, cx| {
                Theme::change(ThemeMode::from(window.appearance()), Some(window), cx);
                Theme::global_mut(cx).background = transparent_black();
                Theme::global_mut(cx).font_size = px(13.);
                Theme::global_mut(cx).muted_foreground = if Theme::global(cx).mode.is_dark() {
                    rgb(0xb6b6c0).into()
                } else {
                    rgb(0x60606a).into()
                };
                window.set_window_title("Quick PR");
                native::install_glass(window);
                let view = cx.new(|cx| ui::QuickPr::new(shell, demo, window, cx));
                cx.set_global(Panel(view.clone()));
                let root = cx.new(|cx| Root::new(view.clone(), window, cx));
                if !background {
                    view.update(cx, |v, cx| v.show(window, cx));
                }
                root
            },
        );
        if let Err(e) = opened {
            eprintln!("Could not open Quick PR: {e:#}");
            cx.quit();
        }
    });
    Ok(())
}
