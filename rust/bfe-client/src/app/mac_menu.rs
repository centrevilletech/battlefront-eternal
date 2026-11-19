//! The macOS menu bar. winit's own default menu names its items after the program file and has
//! the application menu only; this one carries the game's name and adds the two menus a Mac
//! player reaches for: View (Enter Full Screen, Control-Command-F) and Window (Minimize, Zoom).
//! Every item sends a standard AppKit action, so the window and the application answer them
//! as any Mac window does (and winit reports the result as its usual events).
use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2::sel;
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};
use objc2_foundation::{ns_string, MainThreadMarker, NSString};

fn item(mtm: MainThreadMarker, title: &NSString, action: Option<Sel>, key: &NSString, mask: Option<NSEventModifierFlags>) -> Retained<NSMenuItem> {
    // SAFETY: a plain menu item; the action is looked up along the responder chain when chosen.
    let item = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(mtm.alloc(), title, action, key) };
    if let Some(mask) = mask {
        item.setKeyEquivalentModifierMask(mask);
    }
    item
}

fn submenu(mtm: MainThreadMarker, bar: &NSMenu, title: &NSString) -> Retained<NSMenu> {
    let menu = unsafe { NSMenu::initWithTitle(mtm.alloc(), title) };
    let holder = NSMenuItem::new(mtm);
    holder.setSubmenu(Some(&menu));
    bar.addItem(&holder);
    menu
}

/// Installs the menu bar. Called on the main thread once the application has launched.
pub fn install() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let app = NSApplication::sharedApplication(mtm);
    let bar = NSMenu::new(mtm);
    let command = NSEventModifierFlags::NSEventModifierFlagCommand;

    // The application menu takes its bold title from the bundle; an empty title is right here.
    let game = submenu(mtm, &bar, ns_string!(""));
    game.addItem(&item(mtm, ns_string!("About Battlefront Eternal"), Some(sel!(orderFrontStandardAboutPanel:)), ns_string!(""), None));
    game.addItem(&NSMenuItem::separatorItem(mtm));
    let services = unsafe { NSMenu::initWithTitle(mtm.alloc(), ns_string!("Services")) };
    let services_item = item(mtm, ns_string!("Services"), None, ns_string!(""), None);
    services_item.setSubmenu(Some(&services));
    game.addItem(&services_item);
    game.addItem(&NSMenuItem::separatorItem(mtm));
    game.addItem(&item(mtm, ns_string!("Hide Battlefront Eternal"), Some(sel!(hide:)), ns_string!("h"), None));
    game.addItem(&item(mtm, ns_string!("Hide Others"), Some(sel!(hideOtherApplications:)), ns_string!("h"),
        Some(command | NSEventModifierFlags::NSEventModifierFlagOption)));
    game.addItem(&item(mtm, ns_string!("Show All"), Some(sel!(unhideAllApplications:)), ns_string!(""), None));
    game.addItem(&NSMenuItem::separatorItem(mtm));
    game.addItem(&item(mtm, ns_string!("Quit Battlefront Eternal"), Some(sel!(terminate:)), ns_string!("q"), None));

    // AppKit renames this item to Exit Full Screen while the window is full screen.
    let view = submenu(mtm, &bar, ns_string!("View"));
    view.addItem(&item(mtm, ns_string!("Enter Full Screen"), Some(sel!(toggleFullScreen:)), ns_string!("f"),
        Some(command | NSEventModifierFlags::NSEventModifierFlagControl)));

    let window = submenu(mtm, &bar, ns_string!("Window"));
    window.addItem(&item(mtm, ns_string!("Minimize"), Some(sel!(performMiniaturize:)), ns_string!("m"), None));
    window.addItem(&item(mtm, ns_string!("Zoom"), Some(sel!(performZoom:)), ns_string!(""), None));
    window.addItem(&NSMenuItem::separatorItem(mtm));
    window.addItem(&item(mtm, ns_string!("Bring All to Front"), Some(sel!(arrangeInFront:)), ns_string!(""), None));

    // SAFETY: both menus stay alive in the menu bar the application now owns.
    unsafe {
        app.setServicesMenu(Some(&services));
        app.setWindowsMenu(Some(&window));
    }
    app.setMainMenu(Some(&bar));
}
