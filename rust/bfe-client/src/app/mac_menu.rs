//! The macOS menu bar. winit's own default menu names its items after the program file and has
//! the application menu only; this one carries the game's name and adds the two menus a Mac
//! player reaches for: View (Enter Full Screen, Control-Command-F) and Window (Minimize, Zoom).
//! Every item but Quit sends a standard AppKit action, so the window and the application answer
//! them as any Mac window does (and winit reports the result as its usual events). Quit is the
//! game's own: AppKit's `terminate:` ends the process at once, and the game asks first while it
//! hosts a server, as it does when its window is closed.
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, Sel};
use objc2::{declare_class, msg_send_id, mutability, sel, ClassType, DeclaredClass};
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};
use objc2_foundation::{ns_string, MainThreadMarker, NSString};
use std::sync::atomic::{AtomicBool, Ordering};

/// Quit was chosen in the menu bar (or Command-Q pressed) and the application has not taken it yet.
static QUIT_ASKED: AtomicBool = AtomicBool::new(false);

declare_class!(
    /// The Quit item's target: it only notes the wish, the application's loop does the rest.
    struct QuitTarget;

    unsafe impl ClassType for QuitTarget {
        type Super = NSObject;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "BattlefrontEternalQuitTarget";
    }

    impl DeclaredClass for QuitTarget {}

    unsafe impl QuitTarget {
        #[method(quit:)]
        fn quit(&self, _sender: Option<&AnyObject>) {
            QUIT_ASKED.store(true, Ordering::Relaxed);
        }
    }
);

/// Whether Quit was chosen since the last call.
pub fn quit_asked() -> bool {
    QUIT_ASKED.swap(false, Ordering::Relaxed)
}

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
    let quit = item(mtm, ns_string!("Quit Battlefront Eternal"), Some(sel!(quit:)), ns_string!("q"), None);
    // SAFETY: a menu item does not keep its target, so this one is never released: it lives as
    // long as the menu bar does, which is the application's life.
    unsafe {
        let target: Retained<QuitTarget> = msg_send_id![mtm.alloc::<QuitTarget>(), init];
        let target: &AnyObject = Retained::into_raw(target).cast::<AnyObject>().as_ref().expect("a new object");
        quit.setTarget(Some(target));
    }
    game.addItem(&quit);

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
