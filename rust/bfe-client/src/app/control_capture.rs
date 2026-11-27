//! Translate a raw pointer press or scroll before Slint handles the paused options window.
use bfe_interface::option_pages::CapturePointer;
use winit::{event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent}, keyboard::PhysicalKey};

/// Battlefront II capture and gameplay use the same scroll direction for wheels and touchpads.
pub(super) fn scroll_y(delta: MouseScrollDelta) -> f64 {
    match delta {
        MouseScrollDelta::LineDelta(_, y) => -f64::from(y) * 100.,
        MouseScrollDelta::PixelDelta(p) => -p.y,
    }
}

/// Scroll offset used by the shell, spawn screen and scoreboard.
pub(super) fn menu_scroll_y(delta: MouseScrollDelta) -> f64 {
    match delta {
        MouseScrollDelta::LineDelta(_, y) => -y as f64 * 100.0,
        MouseScrollDelta::PixelDelta(p) => p.y,
    }
}

pub(super) fn battle_scroll_y(delta: MouseScrollDelta, family: bfe_family::GameFamily, overlay_open: bool) -> f64 {
    match family {
        bfe_family::GameFamily::Swbf2 if !overlay_open => scroll_y(delta),
        _ => menu_scroll_y(delta),
    }
}

pub(super) fn visible(state: &super::AppState, shell_options_active: bool) -> bool {
    match state {
        super::AppState::Shell => shell_options_active,
        super::AppState::Battle(battle) => battle.options_open,
        _ => false,
    }
}

pub(super) fn key(physical: PhysicalKey, state: ElementState, repeat: bool, synthetic: bool) -> Option<&'static str> {
    if state != ElementState::Pressed || repeat || synthetic { return None; }
    let PhysicalKey::Code(code) = physical else { return None };
    super::keycode_to_dom_code(code)
}

pub(super) fn pointer(event: &WindowEvent) -> Option<CapturePointer> {
    match event {
        WindowEvent::MouseInput { state: ElementState::Pressed, button, .. } => {
            Some(CapturePointer::Button(match button {
                MouseButton::Left => 0,
                MouseButton::Middle => 1,
                MouseButton::Right => 2,
                MouseButton::Back => 3,
                MouseButton::Forward => 4,
                MouseButton::Other(_) => return None,
            }))
        }
        WindowEvent::MouseWheel { delta, .. } => {
            let y = scroll_y(*delta);
            if y < 0. {
                Some(CapturePointer::Wheel(-1))
            } else if y > 0. {
                Some(CapturePointer::Wheel(1))
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::{
        dpi::PhysicalPosition,
        event::{DeviceId, TouchPhase},
    };

    #[test]
    fn battle_menus_keep_their_scroll_direction_with_either_input_unit() {
        use bfe_family::GameFamily::{Swbf1, Swbf2};
        for y in [-2_f32, 3.] {
            for (delta, menu_y, binding_y) in [
                (MouseScrollDelta::LineDelta(0., y), -f64::from(y) * 100., -f64::from(y) * 100.),
                (MouseScrollDelta::PixelDelta(PhysicalPosition::new(0., f64::from(y))), f64::from(y), -f64::from(y)),
            ] {
                assert_eq!(battle_scroll_y(delta, Swbf2, true), menu_y);
                assert_eq!(battle_scroll_y(delta, Swbf1, true), menu_y);
                assert_eq!(battle_scroll_y(delta, Swbf2, false), binding_y);
                assert_eq!(battle_scroll_y(delta, Swbf1, false), menu_y);
            }
        }
    }

    #[test]
    fn focus_replays_leave_capture_open_for_a_real_press() {
        for physical in [winit::keyboard::KeyCode::AltLeft, winit::keyboard::KeyCode::Tab] {
            let keycode = PhysicalKey::Code(physical);
            assert_eq!(key(keycode, ElementState::Pressed, false, true), None);
            assert_eq!(key(keycode, ElementState::Released, false, false), None);
            assert_eq!(key(keycode, ElementState::Pressed, true, false), None);
            assert_eq!(key(keycode, ElementState::Pressed, false, false), super::super::keycode_to_dom_code(physical));
        }
    }

    #[test]
    fn a_shell_modal_owns_input_even_with_a_capture_on_the_options_stack() {
        let shell = super::super::AppState::Shell;
        assert!(!visible(&shell, false));
        assert!(visible(&shell, true));
    }

    #[test]
    fn pressed_side_buttons_and_both_scroll_units_reach_capture() {
        let device_id = DeviceId::dummy();
        let button = |state| WindowEvent::MouseInput { device_id, state, button: MouseButton::Forward };
        assert_eq!(pointer(&button(ElementState::Pressed)), Some(CapturePointer::Button(4)));
        assert_eq!(pointer(&button(ElementState::Released)), None);
        let wheel = |delta| WindowEvent::MouseWheel { device_id, delta, phase: TouchPhase::Moved };
        assert_eq!(pointer(&wheel(MouseScrollDelta::LineDelta(0., 1.))), Some(CapturePointer::Wheel(-1)));
        assert_eq!(
            pointer(&wheel(MouseScrollDelta::PixelDelta(PhysicalPosition::new(0., -0.25)))),
            Some(CapturePointer::Wheel(1))
        );
    }
}
