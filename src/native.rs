use anyhow::{Context, Result};
use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::{Code, HotKey, Modifiers},
};
use gpui::{App, Window};
use objc2::runtime::AnyClass;
use objc2::{MainThreadMarker, extern_class, extern_methods, rc::Retained};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSAutoresizingMaskOptions, NSColor, NSEvent,
    NSGlassEffectView, NSGlassEffectViewStyle, NSScreen, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSAppleEventManager, NSBundle, NSError, NSObject, NSPoint, NSRect, NSSize};
use objc2_quartz_core::{CALayer, kCACornerCurveContinuous};
use quick_pr::activity::Indicator;
use quick_pr::login_item::{self, LoginItemAction};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::cell::Cell;
use tray_icon::{
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuId, MenuItem},
};
const CORNER_RADIUS: f64 = 22.;
thread_local! { static HAS_SHOWN: Cell<bool> = const { Cell::new(false) }; }
fn should_recenter(first: bool, center: (f64, f64), display: (f64, f64, f64, f64)) -> bool {
    first
        || center.0 < display.0
        || center.0 >= display.0 + display.2
        || center.1 < display.1
        || center.1 >= display.1 + display.3
}
pub fn mouse_button_pressed() -> bool {
    NSEvent::pressedMouseButtons() != 0
}
/// Keep the resident application out of the Dock and the application switcher.
pub fn prepare_app() {
    let mtm = MainThreadMarker::new().expect("Quick PR must start on the main thread");
    NSApplication::sharedApplication(mtm)
        .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
}

/// Ordering out a nonactivating NSPanel returns key focus to the previous app.
pub fn hide_panel(window: &mut Window) {
    if let Some(native) = native_window(window) {
        native.orderOut(None);
    }
}

pub fn panel_visible(window: &Window) -> bool {
    native_window(window).is_some_and(|native| native.isVisible())
}

/// Keep the top edge in place while fitting content on the current display.
pub fn fit_panel_height(window: &Window, content_height: f64, cx: &mut App) {
    let Some(native) = native_window(window) else {
        return;
    };
    let Some(screen) = native.screen() else {
        return;
    };
    let visible = screen.visibleFrame();
    let mut frame = native.frame();
    let height = quick_pr::layout::panel_height(content_height, visible.size.height);
    if (frame.size.height - height).abs() < 1. {
        return;
    }
    frame.origin.y = (frame.origin.y + frame.size.height - height).clamp(
        visible.origin.y,
        visible.origin.y + visible.size.height - height,
    );
    frame.size.height = height;
    // AppKit synchronously calls GPUI's resize callback. Run outside the current
    // window update so that callback can borrow the window and update its layout.
    cx.foreground_executor()
        .spawn(async move { native.setFrame_display(frame, true) })
        .detach();
}

pub fn start_drag(window: &Window) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(native) = native_window(window) else {
        return;
    };
    let Some(event) = NSApplication::sharedApplication(mtm).currentEvent() else {
        return;
    };
    native.setMovable(true);
    // AppKit returns immediately; hand off the original mouse-down event while
    // it is still being dispatched, before any queued mouse-up can arrive.
    native.performWindowDragWithEvent(&event);
}

/// Let AppKit composite a real glass surface beneath the transparent Metal view.
pub fn install_glass(window: &mut Window) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(native) = native_window(window) else {
        return;
    };
    // GPUI's titlebar: None still sets Titled | FullSizeContentView, which
    // leaves an NSThemeFrame outline outside the glass. Use an actual borderless
    // panel, retaining NonactivatingPanel so typing does not activate the app.
    native.setStyleMask(
        NSWindowStyleMask::Borderless
            | NSWindowStyleMask::NonactivatingPanel
            | NSWindowStyleMask::Resizable,
    );
    native.setHasShadow(true);
    let Some(content) = native.contentView() else {
        return;
    };
    // Clip the Metal content with the same continuous curve AppKit uses for
    // glass. A separate GPUI circular radius produces a second visible edge.
    content.setWantsLayer(true);
    if let Some(layer) = content.layer() {
        layer.setCornerRadius(CORNER_RADIUS);
        layer.setCornerCurve(unsafe { kCACornerCurveContinuous });
        layer.setMasksToBounds(true);
    }
    if objc2::runtime::AnyClass::get(c"NSGlassEffectView").is_none() {
        window.set_background_appearance(gpui::WindowBackgroundAppearance::Blurred);
        return;
    }
    let glass = NSGlassEffectView::new(mtm);
    glass.setFrame(content.frame());
    glass.setCornerRadius(CORNER_RADIUS);
    glass.setStyle(NSGlassEffectViewStyle::Regular);
    glass.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    native.setOpaque(false);
    native.setBackgroundColor(Some(&NSColor::clearColor()));
    native.setContentView(Some(&glass));
    glass.setContentView(Some(&content));
    // GPUI seeds the outer AppKit frame's backing layer with alpha 0.0001.
    // Clearing NSWindow.backgroundColor does not clear that existing layer:
    // WindowServer still sees a rectangular surface and caches its shadow.
    if let Some(frame) = unsafe { glass.superview() } {
        frame.setWantsLayer(true);
        if let Some(layer) = frame.layer() {
            layer.setBackgroundColor(None);
            layer.setCornerRadius(CORNER_RADIUS);
            layer.setCornerCurve(unsafe { kCACornerCurveContinuous });
            layer.setMasksToBounds(true);
        }
    }
    glass.layoutSubtreeIfNeeded();
    native.invalidateShadow();

    window.refresh();
}

/// Position only when opening; expanding a visible conversation keeps its location.
pub fn show_panel(window: &mut Window, _cx: &mut App) {
    let Some(native) = native_window(window) else {
        window.activate_window();
        return;
    };
    if !native.isVisible() {
        position_on_cursor_display(&native);
    }
    // GPUI's WindowKind::PopUp is a nonactivating NSPanel. Making that panel key
    // accepts typing while the user's previous application remains active.
    native.makeKeyAndOrderFront(None);
    if let Ok(handle) = HasWindowHandle::window_handle(window)
        && let RawWindowHandle::AppKit(handle) = handle.as_raw()
    {
        // Wrapping the content in AppKit glass changes the window's responder;
        // keyboard events must continue to enter GPUI through its original view.
        let view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
        native.makeFirstResponder(Some(view));
    }
    window.refresh();
}

fn native_window(window: &Window) -> Option<Retained<NSWindow>> {
    MainThreadMarker::new()?;
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    // GPUI supplies a live NSView pointer for this borrowed window handle. The
    // call is main-thread-only, and retaining its NSWindow keeps the result alive.
    let view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    view.window()
}

fn position_on_cursor_display(window: &NSWindow) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let cursor = NSEvent::mouseLocation();
    let screen = NSScreen::screens(mtm)
        .into_iter()
        .find(|screen| {
            let frame = screen.frame();
            cursor.x >= frame.origin.x
                && cursor.x < frame.origin.x + frame.size.width
                && cursor.y >= frame.origin.y
                && cursor.y < frame.origin.y + frame.size.height
        })
        .or_else(|| NSScreen::mainScreen(mtm));
    let Some(screen) = screen else {
        return;
    };
    // AppKit uses logical points with a bottom-left origin for both screen and
    // window frames, including displays left of or below the primary display.
    let screen = screen.visibleFrame();
    let frame = window.frame();
    let first_show = HAS_SHOWN.with(|shown| !shown.replace(true));
    if !should_recenter(
        first_show,
        (
            frame.origin.x + frame.size.width / 2.,
            frame.origin.y + frame.size.height / 2.,
        ),
        (
            screen.origin.x,
            screen.origin.y,
            screen.size.width,
            screen.size.height,
        ),
    ) {
        return;
    }
    let padding = 20.0;
    let left = screen.origin.x + padding;
    let right = (screen.origin.x + screen.size.width - frame.size.width - padding).max(left);
    let bottom = screen.origin.y + padding;
    let top = (screen.origin.y + screen.size.height - frame.size.height - padding).max(bottom);
    let x = (screen.origin.x + (screen.size.width - frame.size.width) / 2.0).clamp(left, right);
    let y = (screen.origin.y + screen.size.height * 0.78 - frame.size.height).clamp(bottom, top);
    window.setFrameOrigin(NSPoint::new(x, y));
}

pub enum Event {
    Toggle,
    Refresh,
    Quit,
}
pub struct Shell {
    tray: TrayIcon,
    _menu: Menu,
    keys: GlobalHotKeyManager,
    hotkey: Option<HotKey>,
    refresh: MenuId,
    quit: MenuId,
    show: MenuId,
    pub warning: Option<String>,
    dot: Retained<CALayer>,
    activity_signature: Option<(usize, Indicator, String)>,
}
impl Shell {
    pub fn new() -> Result<Self> {
        let keys = GlobalHotKeyManager::new()?;
        let key = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyP);
        let (hotkey, warning) = match keys.register(key) {
            Ok(()) => (Some(key), None),
            Err(_) => (
                None,
                Some("⌃⌥P is already in use. Open Quick PR from the menu bar.".into()),
            ),
        };
        let show = MenuItem::new("Show / Hide Quick PR   ⌃⌥P", true, None);
        let refresh = MenuItem::new("Refresh", true, None);
        let quit = MenuItem::new("Quit Quick PR", true, None);
        let menu = Menu::with_items(&[&show, &refresh, &quit])?;
        let mut rgba = vec![0u8; 36 * 36 * 4];
        for y in 0i32..36 {
            for x in 0i32..36 {
                let circle = [(10, 8), (10, 28), (27, 28)].iter().any(|(cx, cy)| {
                    let r = (x - cx).pow(2) + (y - cy).pow(2);
                    (6..=20).contains(&r)
                });
                let stem = (9..=11).contains(&x) && (12..=24).contains(&y);
                let branch = ((25..=27).contains(&x) && (13..=24).contains(&y))
                    || ((16..=25).contains(&x) && (10..=12).contains(&y));
                let arrow = (14..=19).contains(&x) && (y - 11).abs() == x - 14;
                if circle || stem || branch || arrow {
                    rgba[((y * 36 + x) * 4 + 3) as usize] = 255;
                }
            }
        }
        let tray = TrayIconBuilder::new()
            .with_id("quick-pr")
            .with_icon(Icon::from_rgba(rgba, 36, 36)?)
            .with_icon_as_template(true)
            .with_tooltip("Quick PR · ⌃⌥P")
            .with_menu(Box::new(menu.clone()))
            .with_menu_on_left_click(false)
            .build()
            .context("Could not create menu bar icon")?;
        // Keep the PR symbol as an adaptive template image. A separate color layer
        // preserves the activity dot's color without disabling macOS icon tinting.
        let mtm =
            MainThreadMarker::new().context("Menu bar indicator must run on the main thread")?;
        let button = tray
            .ns_status_item()
            .and_then(|item| item.button(mtm))
            .context("Could not find the menu bar button")?;
        button.setWantsLayer(true);
        let layer = button
            .layer()
            .context("Could not create menu bar backing layer")?;
        let dot = CALayer::layer();
        let bounds = button.bounds();
        dot.setFrame(NSRect::new(
            NSPoint::new(bounds.size.width - 8., 3.),
            NSSize::new(6., 6.),
        ));
        dot.setCornerRadius(3.);
        dot.setZPosition(10.);
        dot.setHidden(true);
        layer.addSublayer(&dot);
        Ok(Self {
            tray,
            _menu: menu,
            keys,
            hotkey,
            show: show.id().clone(),
            refresh: refresh.id().clone(),
            quit: quit.id().clone(),
            warning,
            dot,
            activity_signature: None,
        })
    }
    pub fn activity(&mut self, count: usize, indicator: Indicator, tooltip: String) -> Result<()> {
        let signature = (count, indicator, tooltip);
        if self.activity_signature.as_ref() == Some(&signature) {
            return Ok(());
        }
        // The title extends to the right; keep the dot anchored to the PR symbol.
        self.tray.set_title(Some(if count > 0 {
            format!(" {count}")
        } else {
            String::new()
        }));
        let color = match indicator {
            Indicator::Quiet => None,
            Indicator::New => Some((0.18, 0.58, 1.)),
            Indicator::Ready => Some((0.20, 0.80, 0.43)),
            Indicator::Problem => Some((1., 0.29, 0.27)),
        };
        if let Some((r, g, b)) = color {
            let color = NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.);
            self.dot.setBackgroundColor(Some(&color.CGColor()));
        }
        self.dot.setHidden(color.is_none());
        self.tray.set_tooltip(Some(&signature.2))?;
        self.activity_signature = Some(signature);
        Ok(())
    }
    pub fn poll(&self) -> Vec<Event> {
        let mut events = vec![];
        for event in TrayIconEvent::receiver().try_iter() {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                events.push(Event::Toggle);
            }
        }
        for event in GlobalHotKeyEvent::receiver().try_iter() {
            if self.hotkey.is_some_and(|k| k.id() == event.id)
                && event.state == HotKeyState::Pressed
            {
                events.push(Event::Toggle);
            }
        }
        for event in MenuEvent::receiver().try_iter() {
            if event.id == self.show {
                events.push(Event::Toggle);
            } else if event.id == self.refresh {
                events.push(Event::Refresh);
            } else if event.id == self.quit {
                events.push(Event::Quit);
            }
        }
        events
    }
    pub fn show_at_menu(&self, window: &mut Window, cx: &mut App) {
        show_panel(window, cx);
        // AppKit mouse coordinates and window frames share logical points.
        if let Some(native) = native_window(window) {
            let cursor = NSEvent::mouseLocation();
            if let Some(screen) = native.screen() {
                let visible = screen.visibleFrame();
                let frame = native.frame();
                let x = (cursor.x - frame.size.width / 2.).clamp(
                    visible.origin.x,
                    (visible.origin.x + visible.size.width - frame.size.width)
                        .max(visible.origin.x),
                );
                let y = visible.origin.y + visible.size.height - frame.size.height - 8.;
                native.setFrameOrigin(NSPoint::new(x, y.max(visible.origin.y)));
            }
        }
    }
}
impl Drop for Shell {
    fn drop(&mut self) {
        if let Some(key) = self.hotkey {
            let _ = self.keys.unregister(key);
        }
    }
}

extern_class!(
    #[unsafe(super(NSObject))]
    #[derive(PartialEq, Eq, Hash, Debug)]
    struct SMAppService;
);

impl SMAppService {
    extern_methods!(
        #[unsafe(method(mainAppService))]
        fn main_app() -> Retained<Self>;

        #[unsafe(method(status))]
        fn status(&self) -> isize;

        #[unsafe(method(registerAndReturnError:_))]
        fn register_login_item(&self) -> std::result::Result<(), Retained<NSError>>;

        #[unsafe(method(unregisterAndReturnError:_))]
        fn unregister_login_item(&self) -> std::result::Result<(), Retained<NSError>>;
    );
}

pub enum LoginItemSync {
    Updated,
    NeedsApproval,
    NotInstalledApp,
    Unsupported,
    Failed(String),
}

/// Register or remove this app as a macOS login item. Only the installed `.app` can register.
pub fn sync_login_item(enabled: bool) -> LoginItemSync {
    let path = NSBundle::mainBundle().bundlePath().to_string();
    if !login_item::is_app_bundle(&path) {
        return LoginItemSync::NotInstalledApp;
    }
    let Some(service) = login_service() else {
        return LoginItemSync::Unsupported;
    };
    match login_item::login_item_action(enabled, service.status() as i32) {
        LoginItemAction::Keep => LoginItemSync::Updated,
        LoginItemAction::NeedsApproval => LoginItemSync::NeedsApproval,
        LoginItemAction::Register => apply_login_change(&service, true),
        LoginItemAction::Unregister => apply_login_change(&service, false),
    }
}

fn apply_login_change(service: &SMAppService, register: bool) -> LoginItemSync {
    let result = if register {
        service.register_login_item()
    } else {
        service.unregister_login_item()
    };
    match result {
        Ok(()) if register => match login_item::login_item_action(true, service.status() as i32) {
            LoginItemAction::NeedsApproval => LoginItemSync::NeedsApproval,
            LoginItemAction::Keep => LoginItemSync::Updated,
            LoginItemAction::Register | LoginItemAction::Unregister => {
                LoginItemSync::Failed("Could not update open at login.".into())
            }
        },
        Ok(()) => LoginItemSync::Updated,
        Err(error) => {
            let message = error.localizedDescription().to_string();
            LoginItemSync::Failed(if message.is_empty() {
                "Could not update open at login.".into()
            } else {
                format!("Could not update open at login: {message}")
            })
        }
    }
}

fn login_service() -> Option<Retained<SMAppService>> {
    load_service_management();
    AnyClass::get(c"SMAppService")?;
    Some(SMAppService::main_app())
}

fn load_service_management() {
    unsafe extern "C" {
        fn dlopen(path: *const std::ffi::c_char, mode: i32) -> *mut std::ffi::c_void;
    }
    const RTLD_LAZY: i32 = 1;
    const RTLD_GLOBAL: i32 = 8;
    let path = c"/System/Library/Frameworks/ServiceManagement.framework/ServiceManagement";
    unsafe {
        dlopen(path.as_ptr(), RTLD_LAZY | RTLD_GLOBAL);
    }
}

/// Login items launch the app with `kAEOpenApplication` / `keyAELaunchedAsLogInItem`.
/// That event is still current during `applicationDidFinishLaunching`.
pub fn launched_at_login() -> bool {
    if std::env::args().any(|arg| arg == "--background") {
        return true;
    }
    let Some(event) = NSAppleEventManager::sharedAppleEventManager().currentAppleEvent() else {
        return false;
    };
    let property = event
        .paramDescriptorForKeyword(login_item::PROP_DATA)
        .map(|descriptor| descriptor.enumCodeValue());
    login_item::is_login_launch(Some(event.eventID()), property)
}

pub fn notify(message: String) {
    std::thread::spawn(move || {
        let _ = std::process::Command::new("/usr/bin/osascript").args(["-e", "on run argv\ndisplay notification (item 1 of argv) with title \"Quick PR\"\nend run", &message]).output();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_service_reports_a_known_status_without_registering() {
        let service = login_service().expect("SMAppService is available on this Mac");
        let class = AnyClass::get(c"SMAppService").expect("SMAppService");
        assert!(class.class_method(objc2::sel!(mainAppService)).is_some());
        assert!(class.instance_method(objc2::sel!(status)).is_some());
        assert!(
            class
                .instance_method(objc2::sel!(registerAndReturnError:))
                .is_some()
        );
        assert!(
            class
                .instance_method(objc2::sel!(unregisterAndReturnError:))
                .is_some()
        );
        let status = service.status();
        assert!(
            (0..=3).contains(&status),
            "unexpected SMAppService status {status}"
        );
    }
}
