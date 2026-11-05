/// Apple event id `kAEOpenApplication` ('oapp').
pub const OPEN_APPLICATION: u32 = 0x6f61_7070;
/// Apple event keyword `keyAEPropData` ('prdt').
pub const PROP_DATA: u32 = 0x7072_6474;
/// Apple event enum `keyAELaunchedAsLogInItem` ('lgit').
pub const LAUNCHED_AS_LOGIN_ITEM: u32 = 0x6c67_6974;

/// macOS marks a login-item launch as an open-application event with this property.
pub fn is_login_launch(event_id: Option<u32>, property: Option<u32>) -> bool {
    event_id == Some(OPEN_APPLICATION) && property == Some(LAUNCHED_AS_LOGIN_ITEM)
}

/// `SMAppService` can register the main bundle, which is the `.app` directory.
pub fn is_app_bundle(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .is_some_and(|ext| ext == "app")
}

/// What to ask `SMAppService` for, given the saved preference and the current status.
/// Status values match `SMAppServiceStatus`: 0 not registered, 1 enabled, 2 needs approval, 3 not found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginItemAction {
    Keep,
    Register,
    Unregister,
    NeedsApproval,
}

pub fn login_item_action(enabled: bool, status: i32) -> LoginItemAction {
    const ENABLED: i32 = 1;
    const REQUIRES_APPROVAL: i32 = 2;
    if enabled {
        return match status {
            ENABLED => LoginItemAction::Keep,
            REQUIRES_APPROVAL => LoginItemAction::NeedsApproval,
            _ => LoginItemAction::Register,
        };
    }
    if matches!(status, 0 | 3) {
        LoginItemAction::Keep
    } else {
        LoginItemAction::Unregister
    }
}
