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
use slint::{
    platform::{
        femtovg_renderer::{FemtoVGRenderer, OpenGLInterface},
        WindowAdapter, WindowEvent as UiEvent,
    },
    ComponentHandle,
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
    ime: Cell<bool>,
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
        self.cursor.set(icon);
        self.window.set_cursor(icon);
    }
    // winit converts logical caret geometry for the active compositor scale.
    // Preedit and committed text return through the ordinary window event path.
    fn input_method_request(&self, request: InputMethodRequest) {
        self.ime_requests.set(self.ime_requests.get() + 1);
        match request {
            InputMethodRequest::Disable => {
                self.ime.set(false);
                self.window.set_ime_allowed(false);
            }
            InputMethodRequest::Enable(p) | InputMethodRequest::Update(p) => {
                self.ime.set(true);
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
    fn set_clipboard_text(&self, text: &str, _: slint::platform::Clipboard) {
        let mut c = self.clipboard.borrow_mut();
        if c.is_none() {
            *c = arboard::Clipboard::new().ok();
        }
        if let Some(c) = c.as_mut() {
            let _ = c.set_text(text);
        }
    }
    fn clipboard_text(&self, _: slint::platform::Clipboard) -> Option<String> {
        let mut c = self.clipboard.borrow_mut();
        if c.is_none() {
            *c = arboard::Clipboard::new().ok();
        }
        c.as_mut()?.get_text().ok()
    }
}

pub(super) struct SlintHost {
    root: GameUi,
    adapter: Rc<Adapter>,
    underlay: Rc<Cell<Option<(glow::Framebuffer, [i32; 2])>>>,
    pending: Pending,
    quit: Arc<AtomicBool>,
    cursor_position: slint::LogicalPosition,
    pressed: u8,
    preedit: bool,
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
            ime: Cell::new(false),
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
        root.set_proof_controls(proof);
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
        let mut host = Self {
            root,
            adapter,
            underlay,
            pending,
            quit,
            cursor_position: slint::LogicalPosition::default(),
            pressed: 0,
            preedit: false,
            frames: Cell::new(0),
            gl: readback,
            context,
            composited_frames,
        };
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
    pub fn render(&self, fb: glow::Framebuffer, size: [i32; 2]) -> Result<(), String> {
        if !self.context.is_current() {
            return Err("shared GL context is not current".into());
        }
        self.underlay.set(Some((fb, size)));
        self.root.window().request_redraw();
        self.adapter.renderer.render().map_err(|e| e.to_string())?;
        self.frames.set(self.frames.get() + 1);
        Ok(())
    }
    pub fn cursor(&self) -> Option<CursorIcon> {
        self.in_panel().then(|| self.adapter.cursor.get())
    }
    fn in_panel(&self) -> bool {
        self.root.get_proof_controls()
            && !self.root.get_gameplay()
            && (16.0..316.0).contains(&self.cursor_position.x)
            && (16.0..190.0).contains(&self.cursor_position.y)
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
                if !f { self.preedit=false; }
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
                let inside = self.in_panel() || self.pressed & mask != 0;
                if !inside {
                    if *state == ElementState::Pressed {
                        self.root.invoke_release_editor();
                        // Existing native fields own IME until their views move
                        // to Slint. A click returns text ownership to that host.
                        self.adapter.window.set_ime_allowed(true);
                    }
                    return false;
                }
                let down = *state == ElementState::Pressed;
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
                    eprintln!("[slint] activations={} text={:?} cursor={:?}",
                        self.root.get_activations(), self.root.get_draft(), self.adapter.cursor.get());
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
            WindowEvent::Ime(ime) if self.root.get_text_focused() && !locked => {
                match ime {
                    Ime::Preedit(text, range) => {
                        self.preedit = !text.is_empty();
                        self.text_event(InternalKeyEvent {
                            event_type: KeyEventType::UpdateComposition,
                            preedit_text: text.into(),
                            preedit_selection: range.map(|(a, b)| a as i32..b as i32),
                            ..Default::default()
                        });
                    }
                    Ime::Commit(text) => {
                        self.preedit = false;
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
            WindowEvent::KeyboardInput { event, .. } if self.root.get_text_focused() && !locked => {
                if self.preedit {
                    return true;
                }
                let text = key_text(&event.logical_key);
                #[cfg(target_os = "windows")]
                let text_without_modifiers = {
                    use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
                    key_text(&event.key_without_modifiers()).unwrap_or_default()
                };
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
                }
                return true;
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
        self.root.set_gameplay(false);
        self.root.set_proof_controls(true);
        self.root.invoke_focus_editor();
        self.event(
            &WindowEvent::Ime(Ime::Preedit("候補".into(), Some((0, 6)))),
            false,
        );
        if !self.root.get_draft().is_empty() {
            return Err("IME preedit committed too early".into());
        }
        self.event(&WindowEvent::Ime(Ime::Commit("入力é".into())), false);
        if self.root.get_draft().as_str() != "入力é" {
            return Err("IME composition commit lost Unicode".into());
        }
        self.root.window().dispatch_event(UiEvent::PointerMoved {
            position: slint::LogicalPosition::new(40., 125.),
        });
        let report = serde_json::json!({"imeText":self.root.get_draft().as_str(),"imeRequests":self.adapter.ime_requests.get(),"cursorRequests":self.adapter.cursor_requests.get(),"cursor":format!("{:?}",self.adapter.cursor.get()),"sharedContext":self.context.is_current(),"uiFrames":self.frames.get(),"compositedFrames":self.composited_frames.get()});
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
