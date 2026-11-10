//! Slint draws into the game's current GL context; the host alone presents.
//! Keep the version-pinned cursor and composition hooks in this adapter so
//! screen components never duplicate window-system or input-method plumbing.
//!
//! The winit/glutin host owns the only OS window, surface, context and swap.
//! FemtoVG clears its transparent root before notifying BeforeRendering, so
//! copying the completed game target there preserves movies and renderer blur.
//! Both renderers retain resources; native state caches are invalidated when
//! control returns to the game. The notifier restores the state it borrows.
//!
//! Text events use logical keys and Slint composition; battle bindings keep
//! their original physical keys and raw device motion outside this module.
//! Cursor and IME use the same version-pinned hooks as Slint's winit backend.
//! Keep those hooks here when upgrading Slint, rather than in screen code.
use glow::HasContext;
use glutin::{
    context::PossiblyCurrentContext,
    display::GetGlDisplay,
    prelude::*,
    surface::{Surface, WindowSurface},
};
use i_slint_core::{
    cursor::MouseCursorInner,
    input::{InternalKeyEvent, KeyEventType},
    items::BuiltInMouseCursor,
    window::{InputMethodRequest, WindowAdapterInternal},
};
pub(super) use slint::ComponentHandle;
use slint::platform::{
    femtovg_renderer::{FemtoVGRenderer, OpenGLInterface},
    WindowAdapter, WindowEvent as UiEvent,
};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    ffi::{c_void, CStr},
    num::NonZeroU32,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use winit::{
    event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent},
    keyboard::{Key as WinitKey, NamedKey},
    window::{CursorIcon, Window},
};
slint::include_modules!();

type Failure = Box<dyn std::error::Error + Send + Sync>;
type Pending = Arc<Mutex<VecDeque<Box<dyn FnOnce() + Send>>>>;
struct Gl {
    context: Rc<PossiblyCurrentContext>,
}
// get_proc_address forwards to the same live glutin display as the game.
unsafe impl OpenGLInterface for Gl {
    fn ensure_current(&self) -> Result<(), Failure> {
        if self.context.is_current() {
            Ok(())
        } else {
            Err("UI context is not current".into())
        }
    }
    fn get_proc_address(&self, name: &CStr) -> *const c_void {
        self.context.display().get_proc_address(name)
    }
    // Slint must never swap or resize independently of the game frame clock.
    fn swap_buffers(&self) -> Result<(), Failure> {
        Ok(())
    }
    fn resize(&self, _: NonZeroU32, _: NonZeroU32) -> Result<(), Failure> {
        Ok(())
    }
}
struct Adapter {
    ui_window: slint::Window,
    renderer: FemtoVGRenderer,
    window: Arc<Window>,
    // Hold the surface until the UI renderer releases all graphics resources.
    _surface: Rc<Surface<WindowSurface>>,
    cursor: Cell<CursorIcon>,
    cursor_requests: Cell<u64>,
    ime_requests: Cell<u64>,
}
impl WindowAdapter for Adapter {
    fn window(&self) -> &slint::Window {
        &self.ui_window
    }
    fn renderer(&self) -> &dyn slint::platform::Renderer {
        &self.renderer
    }
    fn size(&self) -> slint::PhysicalSize {
        let s = self.window.inner_size();
        slint::PhysicalSize::new(s.width, s.height)
    }
    fn request_redraw(&self) {
        self.window.request_redraw();
    }
    fn set_visible(&self, _: bool) -> Result<(), slint::PlatformError> {
        Ok(())
    }
    fn internal(&self, _: i_slint_core::InternalToken) -> Option<&dyn WindowAdapterInternal> {
        Some(self)
    }
}
impl WindowAdapterInternal for Adapter {
    fn set_mouse_cursor(&self, cursor: MouseCursorInner) {
        self.cursor_requests.set(self.cursor_requests.get() + 1);
        let icon = match cursor {
            MouseCursorInner::BuiltIn(c) => match c {
                BuiltInMouseCursor::Pointer => CursorIcon::Pointer,
                BuiltInMouseCursor::Text => CursorIcon::Text,
                BuiltInMouseCursor::Grab => CursorIcon::Grab,
                BuiltInMouseCursor::Grabbing => CursorIcon::Grabbing,
                BuiltInMouseCursor::Crosshair => CursorIcon::Crosshair,
                BuiltInMouseCursor::NotAllowed => CursorIcon::NotAllowed,
                _ => CursorIcon::Default,
            },
            _ => CursorIcon::Default,
        };
        // The host arbitrates this UI request with the battle's turntable/map
        // cursor. Writing the OS cursor here would bypass its cached owner.
        self.cursor.set(icon);
    }
    // winit converts logical caret geometry for the active compositor scale.
    // Preedit and committed text return through the ordinary window event path.
    fn input_method_request(&self, request: InputMethodRequest) {
        self.ime_requests.set(self.ime_requests.get() + 1);
        match request {
            InputMethodRequest::Disable => {
                self.window.set_ime_allowed(false);
            }
            InputMethodRequest::Enable(p) | InputMethodRequest::Update(p) => {
                self.window.set_ime_allowed(true);
                self.window.set_ime_cursor_area(
                    winit::dpi::LogicalPosition::new(
                        p.cursor_rect_origin.x as f64,
                        p.cursor_rect_origin.y as f64,
                    ),
                    winit::dpi::LogicalSize::new(
                        p.cursor_rect_size.width as f64,
                        p.cursor_rect_size.height as f64,
                    ),
                );
            }
            _ => (),
        }
    }
}
// Use the host's redraw pump instead of starting a second window event loop.
// Cross-thread callbacks wake that pump, then execute on its UI thread.
struct Platform {
    adapter: Rc<Adapter>,
    pending: Pending,
    quit: Arc<AtomicBool>,
    clipboard: RefCell<Option<arboard::Clipboard>>,
}
#[derive(Clone)]
struct Proxy {
    pending: Pending,
    quit: Arc<AtomicBool>,
    window: Arc<Window>,
}
impl slint::platform::EventLoopProxy for Proxy {
    fn quit_event_loop(&self) -> Result<(), slint::EventLoopError> {
        self.quit.store(true, Ordering::Release);
        self.window.request_redraw();
        Ok(())
    }
    fn invoke_from_event_loop(
        &self,
        event: Box<dyn FnOnce() + Send>,
    ) -> Result<(), slint::EventLoopError> {
        self.pending.lock().unwrap().push_back(event);
        self.window.request_redraw();
        Ok(())
    }
}
impl slint::platform::Platform for Platform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.adapter.clone())
    }
    fn new_event_loop_proxy(&self) -> Option<Box<dyn slint::platform::EventLoopProxy>> {
        Some(Box::new(Proxy {
            pending: self.pending.clone(),
            quit: self.quit.clone(),
            window: self.adapter.window.clone(),
        }))
    }
    fn set_clipboard_text(&self, text: &str, destination: slint::platform::Clipboard) {
        let mut c = self.clipboard.borrow_mut();
        if c.is_none() {
            *c = arboard::Clipboard::new().ok();
        }
        if let Some(c) = c.as_mut() {
            #[cfg(target_os = "linux")]
            {
                use arboard::{LinuxClipboardKind, SetExtLinux};
                let kind = if destination == slint::platform::Clipboard::SelectionClipboard {
                    LinuxClipboardKind::Primary
                } else {
                    LinuxClipboardKind::Clipboard
                };
                let _ = c.set().clipboard(kind).text(text);
            }
            #[cfg(not(target_os = "linux"))]
            if destination == slint::platform::Clipboard::DefaultClipboard {
                let _ = c.set_text(text);
            }
        }
    }
    fn clipboard_text(&self, source: slint::platform::Clipboard) -> Option<String> {
        let mut c = self.clipboard.borrow_mut();
        if c.is_none() {
            *c = arboard::Clipboard::new().ok();
        }
        let c = c.as_mut()?;
        #[cfg(target_os = "linux")]
        {
            use arboard::{GetExtLinux, LinuxClipboardKind};
            let kind = if source == slint::platform::Clipboard::SelectionClipboard {
                LinuxClipboardKind::Primary
            } else {
                LinuxClipboardKind::Clipboard
            };
            c.get().clipboard(kind).text().ok()
        }
        #[cfg(not(target_os = "linux"))]
        {
            if source == slint::platform::Clipboard::DefaultClipboard {
                c.get_text().ok()
            } else {
                None
            }
        }
    }
}

use super::{
    settings::AppSettingsStore,
    shell::{ShellHost, ShellTransition},
    slint_mp::MpAction,
    slint_models::sync_rows,
    slint_options::{OptionsModels, OptionsUiAction},
};
use crate::audio::SharedAudioRouter;
use bfe_shell::screens::{PopupState, ScreenId};

pub(super) enum UiAction {
    GateUnlocked,
    RowSelected(usize),
    RowActivated(usize),
    BackActivated,
    ThemeChanged(usize),
    ProfileEdited(String),
    ProfileSaved(String),
    ProfileCancelled,
    DiscordChosen(String),
    SteamDeclined,
    SteamAccepted,
    PopupButtonActivated(usize),
    SoundRequested(&'static str),
    Mp(MpAction),
}

pub(super) struct SlintHost {
    pub(super) root: GameUi,
    pub(super) setup: super::slint_setup::SetupHostState,
    adapter: Rc<Adapter>,
    underlay: Rc<Cell<Option<(glow::Framebuffer, [i32; 2])>>>,
    pending: Pending,
    quit: Arc<AtomicBool>,
    pub(super) actions: Rc<RefCell<Vec<UiAction>>>,
    options_actions: Rc<RefCell<Vec<OptionsUiAction>>>,
    options_models: OptionsModels,
    battle_actions: Rc<RefCell<Vec<super::slint_battle::BattleUiAction>>>,
    battle_lists: super::slint_battle::BattleLists,
    ia_actions: super::slint_ia::IaActionQueue,
    was_profile_editing: bool,
    last_shell_dropdown: bool,
    pub(super) mp_models: super::slint_mp::MpModels,
    // The lists the screens repeat over live as long as the host does (slint_models.rs).
    rows: Rc<slint::VecModel<MenuRowItem>>,
    steam_paragraphs: Rc<slint::VecModel<slint::SharedString>>,
    cursor_position: slint::LogicalPosition,
    pressed: u8,
    pub frames: Cell<u64>,
    gl: Rc<glow::Context>,
    context: Rc<PossiblyCurrentContext>,
    composited_frames: Rc<Cell<u64>>,
}
impl SlintHost {
    pub fn new(
        window: Arc<Window>,
        context: Rc<PossiblyCurrentContext>,
        surface: Rc<Surface<WindowSurface>>,
        proof: bool,
    ) -> Result<Self, String> {
        let renderer = FemtoVGRenderer::new(Gl {
            context: context.clone(),
        })
        .map_err(|e| e.to_string())?;
        let adapter: Rc<Adapter> = Rc::new_cyclic(|weak: &std::rc::Weak<Adapter>| Adapter {
            ui_window: slint::Window::new(weak.clone()),
            renderer,
            window: window.clone(),
            _surface: surface,
            cursor: Cell::new(CursorIcon::Default),
            cursor_requests: Cell::new(0),
            ime_requests: Cell::new(0),
        });
        let pending: Pending = Default::default();
        let quit = Arc::new(AtomicBool::new(false));
        slint::platform::set_platform(Box::new(Platform {
            adapter: adapter.clone(),
            pending: pending.clone(),
            quit: quit.clone(),
            clipboard: RefCell::new(None),
        }))
        .map_err(|e| e.to_string())?;
        let root = GameUi::new().map_err(|e| e.to_string())?;
        // Handed to the screens once; `sync_shell` only edits their rows afterwards.
        let rows = Rc::new(slint::VecModel::<MenuRowItem>::default());
        root.set_rows(slint::ModelRc::from(rows.clone()));
        let steam_paragraphs = Rc::new(slint::VecModel::<slint::SharedString>::default());
        root.set_steam_paragraphs(slint::ModelRc::from(steam_paragraphs.clone()));
        root.set_proof_controls(proof);
        let actions: Rc<RefCell<Vec<UiAction>>> = Rc::new(RefCell::new(Vec::new()));
        let options_actions: Rc<RefCell<Vec<OptionsUiAction>>> = Rc::new(RefCell::new(Vec::new()));
        super::slint_options::register_callbacks(&root, options_actions.clone());
        let options_models = OptionsModels::install(&root);
        let battle_actions = Rc::new(RefCell::new(Vec::new()));
        super::slint_battle::register_battle_callbacks(&root, &battle_actions);
        let battle_lists = super::slint_battle::BattleLists::new(&root);
        let ia_actions = super::slint_ia::register_ia(&root);
        {
            let theme = root.global::<Theme>();
            theme.on_format_label(|text, uppercase| bfe_ui::format_label(&text, uppercase).into());
            let q = actions.clone();
            theme.on_sound_requested(move |sound| {
                let cue = match sound {
                    MenuSound::Select => "shell_select_change",
                    MenuSound::Accept => "shell_menu_accept",
                    MenuSound::Back => "shell_menu_cancel",
                    MenuSound::Error => "shell_menu_error",
                };
                q.borrow_mut().push(UiAction::SoundRequested(cue));
            });
        }
        {
            let q = actions.clone();
            root.on_gate_unlocked(move || q.borrow_mut().push(UiAction::GateUnlocked));
        }
        {
            let q = actions.clone();
            root.on_row_selected(move |idx| {
                if idx >= 0 {
                    q.borrow_mut().push(UiAction::RowSelected(idx as usize));
                }
            });
        }
        {
            let q = actions.clone();
            root.on_row_activated(move |idx| {
                if idx >= 0 {
                    q.borrow_mut().push(UiAction::RowActivated(idx as usize));
                }
            });
        }
        {
            let q = actions.clone();
            root.on_back_activated(move || q.borrow_mut().push(UiAction::BackActivated));
        }
        {
            let q = actions.clone();
            root.on_theme_changed(move |idx| {
                if idx >= 0 {
                    q.borrow_mut().push(UiAction::ThemeChanged(idx as usize));
                }
            });
        }
        {
            let q = actions.clone();
            root.on_profile_edited(move |text| {
                q.borrow_mut().push(UiAction::ProfileEdited(text.into()));
            });
        }
        {
            let q = actions.clone();
            root.on_profile_saved(move |text| {
                q.borrow_mut().push(UiAction::ProfileSaved(text.into()));
            });
        }
        {
            let q = actions.clone();
            root.on_profile_cancelled(move || q.borrow_mut().push(UiAction::ProfileCancelled));
        }
        {
            let q = actions.clone();
            root.on_discord_chosen(move |detail| {
                q.borrow_mut().push(UiAction::DiscordChosen(detail.into()));
            });
        }
        {
            let q = actions.clone();
            root.on_steam_declined(move || q.borrow_mut().push(UiAction::SteamDeclined));
        }
        {
            let q = actions.clone();
            root.on_steam_accepted(move || q.borrow_mut().push(UiAction::SteamAccepted));
        }
        {
            let q = actions.clone();
            root.on_popup_button_activated(move |idx| {
                if idx >= 0 {
                    q.borrow_mut()
                        .push(UiAction::PopupButtonActivated(idx as usize));
                }
            });
        }
        let underlay: Rc<Cell<Option<(glow::Framebuffer, [i32; 2])>>> = Rc::new(Cell::new(None));
        let source = underlay.clone();
        let os = window.clone();
        let gl = Rc::new(unsafe {
            glow::Context::from_loader_function_cstr(|name| {
                context.display().get_proc_address(name)
            })
        });
        let readback = gl.clone();
        let composited_frames = Rc::new(Cell::new(0u64));
        let composited = composited_frames.clone();
        root.window()
            .set_rendering_notifier(move |state, _| {
                if matches!(state, slint::RenderingState::BeforeRendering) {
                    if let Some((fb, size)) = source.get() {
                        unsafe {
                            let read = gl.get_parameter_i32(glow::READ_FRAMEBUFFER_BINDING);
                            let draw = gl.get_parameter_i32(glow::DRAW_FRAMEBUFFER_BINDING);
                            let scissor = gl.is_enabled(glow::SCISSOR_TEST);
                            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(fb));
                            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, None);
                            gl.disable(glow::SCISSOR_TEST);
                            composited.set(composited.get() + 1);
                            let target = os.inner_size();
                            gl.blit_framebuffer(
                                0,
                                0,
                                size[0],
                                size[1],
                                0,
                                0,
                                target.width as i32,
                                target.height as i32,
                                glow::COLOR_BUFFER_BIT,
                                glow::LINEAR,
                            );
                            gl.bind_framebuffer(
                                glow::READ_FRAMEBUFFER,
                                NonZeroU32::new(read as u32).map(glow::NativeFramebuffer),
                            );
                            gl.bind_framebuffer(
                                glow::DRAW_FRAMEBUFFER,
                                NonZeroU32::new(draw as u32).map(glow::NativeFramebuffer),
                            );
                            if scissor {
                                gl.enable(glow::SCISSOR_TEST);
                            }
                        }
                    }
                }
            })
            .map_err(|e| format!("UI rendering notifier: {e:?}"))?;
        root.show().map_err(|e| e.to_string())?;
        let mp_models = super::slint_mp::MpModels::install(&root);
        let mut host = Self {
            root,
            setup: Default::default(),
            adapter,
            underlay,
            pending,
            quit,
            actions,
            options_actions,
            options_models,
            battle_actions,
            battle_lists,
            ia_actions,
            was_profile_editing: false,
            last_shell_dropdown: false,
            mp_models,
            rows,
            steam_paragraphs,
            cursor_position: slint::LogicalPosition::default(),
            pressed: 0,
            frames: Cell::new(0),
            gl: readback,
            context,
            composited_frames,
        };
        host.register_setup_callbacks();
        host.register_mp_callbacks();
        host.resize();
        Ok(host)
    }
    pub fn update(&self, gameplay: bool) {
        if gameplay && !self.root.get_gameplay() {
            self.root.invoke_release_editor();
        }
        self.root.set_gameplay(gameplay);
        loop {
            let event = self.pending.lock().unwrap().pop_front();
            if let Some(event) = event {
                event();
            } else {
                break;
            }
        }
        slint::platform::update_timers_and_animations();
    }
    pub fn wants_quit(&self) -> bool {
        self.quit.load(Ordering::Acquire)
    }
    // The native framebuffer stays physical. Slint alone receives logical UI
    // dimensions, including compositor changes while the window is already open.
    pub fn resize(&mut self) {
        let scale = self.adapter.window.scale_factor() as f32;
        let s = self.adapter.window.inner_size();
        self.root
            .window()
            .dispatch_event(UiEvent::ScaleFactorChanged {
                scale_factor: scale,
            });
        self.root.window().dispatch_event(UiEvent::Resized {
            size: slint::LogicalSize::new(s.width as f32 / scale, s.height as f32 / scale),
        });
    }
    pub fn set_shell_visible(&mut self, visible: bool) {
        if !visible && self.was_profile_editing {
            self.root.invoke_release_editor();
            self.was_profile_editing = false;
        }
        self.root.set_shell_visible(visible);
    }

    pub fn sync_battle(&mut self, battle: &mut super::battle::BattleController) {
        super::slint_battle::sync_battle(&self.root, &self.battle_lists, battle);
    }

    pub fn drain_battle_actions(
        &mut self,
        battle: &mut super::battle::BattleController,
        settings: &mut AppSettingsStore,
    ) {
        super::slint_battle::drain_battle_actions(&self.battle_actions, battle, settings);
    }

    pub fn sync_shell(&mut self, shell: &mut ShellHost, visible: bool) {
        self.set_shell_visible(visible);
        super::slint_ia::sync_ia(&self.root, &self.ia_actions, shell, visible);
        if !visible {
            return;
        }
        let (menu_theme, theme_idx) = match shell.app.settings.menu_theme.as_str() {
            "bf1" => ("bf1", 0),
            "bf2" => ("bf2", 1),
            _ => ("eternal", 2),
        };
        self.root.global::<Theme>().set_menu_theme(menu_theme.into());
        self.root.set_theme_index(theme_idx);

        if shell.page.dropdown != self.last_shell_dropdown {
            self.root.set_theme_picker_open(shell.page.dropdown);
            self.last_shell_dropdown = shell.page.dropdown;
        } else {
            shell.page.dropdown = self.root.get_theme_picker_open();
            self.last_shell_dropdown = shell.page.dropdown;
        }

        let id = if shell.app.current_screen() == ScreenId::Popup {
            shell.app.stack.last().copied().unwrap_or(ScreenId::Main)
        } else {
            shell.app.current_screen()
        };
        let screen_str = match id {
            ScreenId::Gate => "gate",
            ScreenId::Main => "main",
            ScreenId::Profile => "profile",
            ScreenId::Options | ScreenId::OptionsPage(_) => "options",
            ScreenId::Singleplayer => "sp",
            ScreenId::InstantAction => "ia",
            ScreenId::InstantActionEra => "ia-era",
            _ => "other",
        };
        self.root.set_active_screen(screen_str.into());

        let pulse = 0.3
            + 0.7 * ((shell.app.gate_elapsed / 1.4 * std::f64::consts::TAU).cos() + 1.0) / 2.0;
        self.root.set_prompt_opacity(pulse as f32);
        self.root
            .set_version_text(shell.app.version_text.as_str().into());

        if matches!(id, ScreenId::Main | ScreenId::Profile) {
            let rows = shell.app.active_rows();
            let model: Vec<MenuRowItem> = rows
                .iter()
                .map(|r| MenuRowItem {
                    id: r.id.as_str().into(),
                    label: r.label.as_str().into(),
                    disabled: r.disabled,
                })
                .collect();
            sync_rows(&self.rows, &model);
            self.root
                .set_selected_row(shell.app.selected_cached_row_index(id) as i32);
        } else if matches!(id, ScreenId::Options | ScreenId::OptionsPage(_)) {
            super::slint_options::sync(&self.root, shell, &self.options_models);
        }

        let editing = shell.app.profile.editing;
        self.root.set_profile_editing(editing);
        self.root
            .set_profile_note(shell.app.profile.message.as_str().into());
        if editing && !self.was_profile_editing {
            self.root
                .set_profile_draft(shell.app.profile.draft.as_str().into());
            self.root.invoke_focus_profile_input();
        } else if !editing && self.was_profile_editing {
            self.root.invoke_release_editor();
        } else if editing && self.root.get_profile_draft().as_str() != shell.app.profile.draft {
            self.root
                .set_profile_draft(shell.app.profile.draft.as_str().into());
        }
        self.was_profile_editing = editing;

        self.root.set_discord_offer_open(shell.page.discord_offer);
        self.root
            .set_discord_selected_button(shell.page.discord_cursor as i32);

        self.root.set_steam_offer_open(shell.steam_offer.open);
        if shell.steam_offer.open {
            let paras: Vec<slint::SharedString> = shell
                .steam_offer
                .paragraphs
                .iter()
                .map(|s| s.as_str().into())
                .collect();
            sync_rows(&self.steam_paragraphs, &paras);
            self.root
                .set_steam_status_text(shell.steam_offer.status_text.as_str().into());
            self.root
                .set_steam_status_error(shell.steam_offer.status_error);
            self.root
                .set_steam_accept_label(shell.steam_offer.accept_label.as_str().into());
            self.root.set_steam_busy(shell.steam_offer.busy);
            self.root
                .set_steam_selected_button(shell.steam_offer.cursor as i32);
        }

        match &shell.app.popup {
            Some(PopupState::Notice { message }) => {
                self.root.set_popup_open(true);
                self.root.set_popup_message(message.as_str().into());
                self.root.set_popup_accept_label("OK".into());
                self.root.set_popup_middle_label("".into());
                self.root.set_popup_cancel_label("".into());
                self.root.set_popup_selected_button(0);
            }
            Some(PopupState::ConfirmExit {
                message,
                selected_yes,
            }) => {
                self.root.set_popup_open(true);
                self.root.set_popup_message(message.as_str().into());
                self.root.set_popup_accept_label("YES".into());
                self.root.set_popup_middle_label("".into());
                self.root.set_popup_cancel_label("NO".into());
                self.root
                    .set_popup_selected_button(i32::from(!*selected_yes));
            }
            Some(PopupState::HostClosePrompt {
                message,
                buttons,
                selected,
                ..
            }) => {
                self.root.set_popup_open(true);
                self.root.set_popup_message(message.as_str().into());
                self.root.set_popup_accept_label(buttons[0].as_str().into());
                self.root.set_popup_middle_label(buttons[1].as_str().into());
                self.root
                    .set_popup_cancel_label(buttons[2].as_str().into());
                self.root.set_popup_selected_button(*selected as i32);
            }
            _ => {
                self.root.set_popup_open(false);
            }
        }
        self.sync_mp(shell);
    }

    pub fn drain_actions(
        &mut self,
        shell: &mut ShellHost,
        settings: &mut AppSettingsStore,
        audio_router: &SharedAudioRouter,
    ) -> Option<ShellTransition> {
        let mut transition = super::slint_options::drain_actions(
            &self.options_actions,
            shell,
            settings,
            audio_router,
        );
        transition = transition.or(
            super::slint_ia::drain_ia(&self.ia_actions, shell, settings, audio_router));
        let pending: Vec<UiAction> = self.actions.borrow_mut().drain(..).collect();
        if pending.is_empty() {
            return transition;
        }
        for action in pending {
            let next = match action {
                UiAction::GateUnlocked => {
                    if shell.app.current_screen() == ScreenId::Gate {
                        shell.app.unlock_gate();
                    }
                    shell.flush_events(settings, audio_router)
                }
                UiAction::RowSelected(index) => {
                    let screen = shell.app.current_screen();
                    shell.app.set_selected_row_index(screen, index);
                    shell.flush_events(settings, audio_router)
                }
                UiAction::RowActivated(index) => {
                    let screen = shell.app.current_screen();
                    shell.app.set_selected_row_index(screen, index);
                    shell.handle_key("Enter", settings, audio_router)
                }
                UiAction::BackActivated => shell.handle_key("Escape", settings, audio_router),
                UiAction::ThemeChanged(index) => {
                    let theme = ["bf1", "bf2", "eternal"][index.min(2)];
                    shell.app.settings.menu_theme = theme.into();
                    shell
                        .options_ctrl
                        .settings
                        .set("menuTheme", shell.app.settings.menu_theme.clone());
                    shell.page.dropdown = false;
                    self.last_shell_dropdown = false;
                    self.root.set_theme_picker_open(false);
                    shell.flush_events(settings, audio_router)
                }
                UiAction::ProfileEdited(text) => {
                    let clean: String =
                        text.chars().filter(|c| !c.is_control()).take(32).collect();
                    shell.app.profile.draft = clean.clone();
                    shell.app.profile.caret = clean.chars().count();
                    shell.app.profile.selected_all = false;
                    if clean != text {
                        self.root.set_profile_draft(clean.into());
                    }
                    None
                }
                UiAction::ProfileSaved(text) => {
                    let clean: String =
                        text.chars().filter(|c| !c.is_control()).take(32).collect();
                    shell.app.profile.draft = clean;
                    shell.handle_key("Enter", settings, audio_router)
                }
                UiAction::ProfileCancelled => shell.handle_key("Escape", settings, audio_router),
                UiAction::DiscordChosen(detail) => {
                    shell.save_discord_offer(settings, &detail);
                    shell.flush_events(settings, audio_router)
                }
                UiAction::SteamDeclined => {
                    shell.decline_steam_offer(settings);
                    shell.flush_events(settings, audio_router)
                }
                UiAction::SteamAccepted => {
                    shell.accept_steam_offer(settings);
                    shell.flush_events(settings, audio_router)
                }
                UiAction::PopupButtonActivated(index) => {
                    match shell.app.popup.as_mut() {
                        Some(
                            PopupState::ConfirmExit { selected_yes, .. }
                            | PopupState::OnlineNotice {
                                selected_accept: selected_yes,
                                ..
                            },
                        ) => *selected_yes = index == 0,
                        Some(PopupState::HostClosePrompt { selected, .. }) => *selected = index,
                        _ => {}
                    }
                    shell.handle_key("Enter", settings, audio_router)
                }
                UiAction::SoundRequested(cue) => {
                    if shell.app.sound_queue.last().copied() != Some(cue) {
                        shell.app.sound_queue.push(cue);
                    }
                    shell.flush_events(settings, audio_router)
                }
                UiAction::Mp(mp_action) => {
                    self.drain_mp_action(mp_action, shell, settings, audio_router)
                }
            };
            if next.is_some() {
                transition = next;
            }
        }
        transition
    }

    pub fn modal_open(&self) -> bool {
        self.root.get_shell_visible()
            && !self.root.get_gameplay()
            && !self.root.get_proof_controls()
            && self.root.get_modal_open()
    }
    pub fn battle_modal_open(&self) -> bool {
        self.root.get_gameplay()
            && !self.root.get_proof_controls()
            && super::slint_battle::battle_modal_open(&self.root)
    }
    pub fn render(&self, fb: glow::Framebuffer, size: [i32; 2]) -> Result<(), String> {
        self.render_pass(fb, size, 0)
    }
    pub fn render_pass(&self, fb: glow::Framebuffer, size: [i32; 2], pass: i32) -> Result<(), String> {
        if !self.context.is_current() {
            return Err("shared GL context is not current".into());
        }
        if self.root.get_render_pass() != pass {
            self.root.set_render_pass(pass);
        }
        self.underlay.set(Some((fb, size)));
        self.root.window().request_redraw();
        self.adapter.renderer.render().map_err(|e| e.to_string())?;
        if pass != 0 {
            self.root.set_render_pass(0);
        }
        let copy_back = ((self.root.get_shell_visible()
            || self.setup_visible()
            || self.root.global::<IaModel>().get_loading_visible())
            && !self.root.get_gameplay()
            && !self.root.get_proof_controls())
            || (self.root.get_gameplay()
                && super::slint_battle::battle_overlay_visible(&self.root));
        if copy_back {
            unsafe {
                let read = self.gl.get_parameter_i32(glow::READ_FRAMEBUFFER_BINDING);
                let draw = self.gl.get_parameter_i32(glow::DRAW_FRAMEBUFFER_BINDING);
                let scissor = self.gl.is_enabled(glow::SCISSOR_TEST);
                self.gl.bind_framebuffer(glow::READ_FRAMEBUFFER, None);
                self.gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(fb));
                self.gl.disable(glow::SCISSOR_TEST);
                let target = self.adapter.window.inner_size();
                self.gl.blit_framebuffer(
                    0,
                    0,
                    target.width as i32,
                    target.height as i32,
                    0,
                    0,
                    size[0],
                    size[1],
                    glow::COLOR_BUFFER_BIT,
                    glow::LINEAR,
                );
                self.gl.bind_framebuffer(
                    glow::READ_FRAMEBUFFER,
                    NonZeroU32::new(read as u32).map(glow::NativeFramebuffer),
                );
                self.gl.bind_framebuffer(
                    glow::DRAW_FRAMEBUFFER,
                    NonZeroU32::new(draw as u32).map(glow::NativeFramebuffer),
                );
                if scissor {
                    self.gl.enable(glow::SCISSOR_TEST);
                }
            }
        }
        if pass != 1 {
            self.frames.set(self.frames.get() + 1);
        }
        Ok(())
    }
    pub fn cursor(&self) -> Option<CursorIcon> {
        self.in_panel().then(|| self.adapter.cursor.get())
    }
    fn in_panel(&self) -> bool {
        self.root
            .invoke_controls_at(self.cursor_position.x, self.cursor_position.y)
            || self.mp_controls_active()
    }
    fn text_event(&mut self, event: InternalKeyEvent) {
        self.root.window().dispatch_event(UiEvent::internal(event));
    }
    /// UI input is logical; combat continues to receive the original winit event.
    /// In particular physical bindings, XI2/raw motion and extra buttons never
    /// travel through Slint's text-oriented key event representation.
    pub fn event(&mut self, event: &WindowEvent, locked: bool) -> bool {
        match event {
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => self.resize(),
            WindowEvent::Focused(f) => {
                self.root
                    .window()
                    .dispatch_event(UiEvent::WindowActiveChanged(*f));
            }
            WindowEvent::CursorMoved { position, .. } if !locked => {
                let s = self.adapter.window.scale_factor();
                self.cursor_position =
                    slint::LogicalPosition::new((position.x / s) as f32, (position.y / s) as f32);
                self.root.window().dispatch_event(UiEvent::PointerMoved {
                    position: self.cursor_position,
                });
            }
            WindowEvent::CursorLeft { .. } => {
                self.root.window().dispatch_event(UiEvent::PointerExited);
            }
            WindowEvent::MouseInput { state, button, .. } if !locked => {
                let (b, mask) = match button {
                    MouseButton::Left => (slint::platform::PointerEventButton::Left, 1),
                    MouseButton::Middle => (slint::platform::PointerEventButton::Middle, 2),
                    MouseButton::Right => (slint::platform::PointerEventButton::Right, 4),
                    _ => return false,
                };
                let down = *state == ElementState::Pressed;
                let inside = if down {
                    self.in_panel()
                } else {
                    self.pressed & mask != 0
                };
                if !inside {
                    if down {
                        self.root.invoke_release_editor();
                        // Existing native fields own IME until their views move
                        // to Slint. A click returns text ownership to that host.
                        self.adapter.window.set_ime_allowed(true);
                    }
                    return false;
                }
                self.pressed = if down {
                    self.pressed | mask
                } else {
                    self.pressed & !mask
                };
                self.root.window().dispatch_event(if down {
                    UiEvent::PointerPressed {
                        position: self.cursor_position,
                        button: b,
                    }
                } else {
                    UiEvent::PointerReleased {
                        position: self.cursor_position,
                        button: b,
                    }
                });
                if !down && self.root.get_proof_controls() {
                    eprintln!(
                        "[slint] activations={} text={:?} cursor={:?}",
                        self.root.get_activations(),
                        self.root.get_draft(),
                        self.adapter.cursor.get()
                    );
                }
                return true;
            }
            WindowEvent::MouseWheel { delta, .. } if !locked && self.in_panel() => {
                let (x, y) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (*x * 40., *y * 40.),
                    MouseScrollDelta::PixelDelta(p) => {
                        let s = self.adapter.window.scale_factor();
                        ((p.x / s) as f32, (p.y / s) as f32)
                    }
                };
                self.root.window().dispatch_event(UiEvent::PointerScrolled {
                    position: self.cursor_position,
                    delta_x: x,
                    delta_y: y,
                });
                return true;
            }
            WindowEvent::Ime(ime)
                if (self.root.get_text_focused() || self.mp_text_focused()) && !locked =>
            {
                match ime {
                    Ime::Preedit(text, range) => {
                        self.text_event(InternalKeyEvent {
                            event_type: KeyEventType::UpdateComposition,
                            preedit_text: text.into(),
                            preedit_selection: range.map(|(a, b)| a as i32..b as i32),
                            ..Default::default()
                        });
                    }
                    Ime::Commit(text) => {
                        let mut ev = InternalKeyEvent {
                            event_type: KeyEventType::CommitComposition,
                            ..Default::default()
                        };
                        ev.key_event.text = text.into();
                        self.text_event(ev);
                    }
                    _ => (),
                }
                return true;
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } if !locked
                && !((self.root.get_profile_editing() || self.mp_text_focused())
                    && matches!(event.logical_key, WinitKey::Named(NamedKey::Escape)))
                && (self.root.get_text_focused()
                    || self.root.get_profile_editing()
                    || self.mp_text_focused()
                    || is_modifier(&event.logical_key)) =>
            {
                let focused = self.root.get_text_focused()
                    || self.root.get_profile_editing()
                    || self.mp_text_focused();
                // Focus acquisition replays held keys. Only modifiers belong in
                // the UI state; replayed character keys must not insert text.
                if *is_synthetic && !is_modifier(&event.logical_key) {
                    return focused;
                }
                #[allow(unused_mut)]
                let mut text = key_text(&event.logical_key);
                #[cfg(target_os = "windows")]
                let text_without_modifiers = {
                    use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
                    key_text(&event.key_without_modifiers()).unwrap_or_default()
                };
                #[cfg(target_os = "windows")]
                if text.is_none()
                    && !text_without_modifiers.is_empty()
                    && !matches!(event.logical_key, WinitKey::Dead(_))
                {
                    text = Some(text_without_modifiers.clone());
                }
                if let Some(text) = text {
                    let down = event.state == ElementState::Pressed;
                    let mut e = InternalKeyEvent {
                        event_type: if down {
                            KeyEventType::KeyPressed
                        } else {
                            KeyEventType::KeyReleased
                        },
                        ..Default::default()
                    };
                    e.key_event.text = text;
                    e.key_event.repeat = event.repeat;
                    #[cfg(target_os = "windows")]
                    {
                        e.text_without_modifiers = text_without_modifiers;
                    }
                    self.text_event(e);
                    return focused;
                }
                return false;
            }
            _ => (),
        }
        false
    }
    /// Read the composited backbuffer before the host's sole swap, for platform
    /// smoke evidence. The scene's offscreen readback cannot see the UI layer.
    pub fn pixels(&self) -> Vec<u8> {
        let size = self.adapter.window.inner_size();
        let mut pixels = vec![0; size.width as usize * size.height as usize * 4];
        unsafe {
            self.gl.bind_framebuffer(glow::READ_FRAMEBUFFER, None);
            self.gl.read_pixels(
                0,
                0,
                size.width as i32,
                size.height as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
        }
        let stride = size.width as usize * 4;
        for y in 0..size.height as usize / 2 {
            let opposite = size.height as usize - 1 - y;
            let (top, bottom) = pixels.split_at_mut(opposite * stride);
            top[y * stride..(y + 1) * stride].swap_with_slice(&mut bottom[..stride]);
        }
        pixels
    }
    // Platform CI cannot depend on an installed desktop IME. Exercise its
    // preedit/commit protocol through the real adapter and a compiled TextInput.
    // Real keyboard, cursor and pointer-lock runs complement this smoke.
    pub fn prove_input(&mut self) -> Result<serde_json::Value, String> {
        self.root.set_shell_visible(false);
        self.root.set_gameplay(false);
        self.root.set_proof_controls(true);
        self.root.invoke_focus_editor();
        self.event(
            &WindowEvent::Ime(Ime::Preedit("\u{5019}\u{88dc}".into(), Some((0, 6)))),
            false,
        );
        if !self.root.get_draft().is_empty() {
            return Err("IME preedit committed too early".into());
        }
        self.event(&WindowEvent::Ime(Ime::Commit("\u{5165}\u{529b}\u{00e9}".into())), false);
        if self.root.get_draft().as_str() != "\u{5165}\u{529b}\u{00e9}" {
            return Err("IME composition commit lost Unicode".into());
        }
        let mut shapes = Vec::new();
        for (x, y, expected) in [
            (40., 70., CursorIcon::Text),
            (40., 125., CursorIcon::Pointer),
            (200., 125., CursorIcon::Grab),
        ] {
            self.cursor_position = slint::LogicalPosition::new(x, y);
            self.root.window().dispatch_event(UiEvent::PointerMoved {
                position: self.cursor_position,
            });
            let cursor = self.cursor().ok_or("proof pointer outside UI controls")?;
            if cursor != expected {
                return Err(format!("UI requested {cursor:?}, expected {expected:?}"));
            }
            self.adapter.window.set_cursor(cursor);
            shapes.push(format!("{cursor:?}"));
        }
        self.root.window().dispatch_event(UiEvent::PointerPressed {
            position: self.cursor_position,
            button: slint::platform::PointerEventButton::Left,
        });
        // Cursor bindings are sampled during pointer traversal. Exercise an
        // actual drag motion after the press changes TouchArea.pressed.
        self.root.window().dispatch_event(UiEvent::PointerMoved {
            position: self.cursor_position,
        });
        if self.adapter.cursor.get() != CursorIcon::Grabbing {
            return Err("pressed UI drag cursor missing".into());
        }
        shapes.push("Grabbing".to_string());
        self.adapter.window.set_cursor(CursorIcon::Grabbing);
        self.root.window().dispatch_event(UiEvent::PointerReleased {
            position: self.cursor_position,
            button: slint::platform::PointerEventButton::Left,
        });
        self.cursor_position = slint::LogicalPosition::new(40., 125.);
        self.root.window().dispatch_event(UiEvent::PointerMoved {
            position: self.cursor_position,
        });
        self.adapter.window.set_cursor(self.adapter.cursor.get());
        let report = serde_json::json!({"imeText":self.root.get_draft().as_str(),"imeRequests":self.adapter.ime_requests.get(),"cursorRequests":self.adapter.cursor_requests.get(),"cursorShapes":shapes,"cursor":format!("{:?}",self.adapter.cursor.get()),"sharedContext":self.context.is_current(),"uiFrames":self.frames.get(),"compositedFrames":self.composited_frames.get()});
        if self.adapter.ime_requests.get() == 0 || self.adapter.cursor.get() != CursorIcon::Pointer
        {
            return Err(format!("UI platform hooks not exercised: {report}"));
        }
        self.root.invoke_release_editor();
        self.adapter.window.set_ime_allowed(true);
        Ok(report)
    }
}
fn key_text(key: &WinitKey) -> Option<slint::SharedString> {
    use slint::platform::Key as K;
    Some(match key {
        WinitKey::Character(s) => s.as_str().into(),
        WinitKey::Named(k) => match k {
            NamedKey::Backspace => K::Backspace.into(),
            NamedKey::Delete => K::Delete.into(),
            NamedKey::Enter => K::Return.into(),
            NamedKey::Tab => K::Tab.into(),
            NamedKey::Escape => K::Escape.into(),
            NamedKey::ArrowLeft => K::LeftArrow.into(),
            NamedKey::ArrowRight => K::RightArrow.into(),
            NamedKey::ArrowUp => K::UpArrow.into(),
            NamedKey::ArrowDown => K::DownArrow.into(),
            NamedKey::Home => K::Home.into(),
            NamedKey::End => K::End.into(),
            NamedKey::Shift => K::Shift.into(),
            NamedKey::Control => K::Control.into(),
            NamedKey::Alt => K::Alt.into(),
            NamedKey::Super => K::Meta.into(),
            NamedKey::AltGraph => K::AltGr.into(),
            NamedKey::PageUp => K::PageUp.into(),
            NamedKey::PageDown => K::PageDown.into(),
            NamedKey::Insert => K::Insert.into(),
            NamedKey::Space => " ".into(),
            _ => return None,
        },
        _ => return None,
    })
}
fn is_modifier(key: &WinitKey) -> bool {
    matches!(
        key,
        WinitKey::Named(
            NamedKey::Shift
                | NamedKey::Control
                | NamedKey::Alt
                | NamedKey::Super
                | NamedKey::AltGraph
        )
    )
}
