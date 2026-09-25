//! Recovering the physical key Slint does not expose.
//!
//! Slint hands the UI only a logical key -- the character the layout produced
//! -- which on macOS means `option+a` arrives as `a-ring` with no way back to
//! `a`. The browser build reads `ev.code` for exactly this case.
//!
//! winit does carry the physical key, and Slint's winit backend lets a
//! `CustomApplicationHandler` see events *before* Slint does. So the physical
//! key is recorded here as each keypress goes past, and read back when the
//! capture UI receives the matching logical event.

use std::cell::RefCell;

use slint::winit_030::{
    winit::{
        event::{ElementState, WindowEvent},
        keyboard::{KeyCode, PhysicalKey},
    },
    CustomApplicationHandler, EventResult,
};

thread_local! {
    /// The physical key of the most recent press, in `ev.code` terms.
    static LAST: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The physical key behind the keypress Slint is currently delivering.
pub fn last() -> Option<String> {
    LAST.with(|c| c.borrow().clone())
}

/// `KeyCode` in the same vocabulary the browser's `ev.code` uses, reduced to
/// what a binding needs: a letter, a digit, a function key, or nothing.
fn name(code: KeyCode) -> Option<String> {
    use KeyCode::*;
    let letter = |c: char| Some(c.to_string());
    Some(match code {
        KeyA => return letter('a'),
        KeyB => return letter('b'),
        KeyC => return letter('c'),
        KeyD => return letter('d'),
        KeyE => return letter('e'),
        KeyF => return letter('f'),
        KeyG => return letter('g'),
        KeyH => return letter('h'),
        KeyI => return letter('i'),
        KeyJ => return letter('j'),
        KeyK => return letter('k'),
        KeyL => return letter('l'),
        KeyM => return letter('m'),
        KeyN => return letter('n'),
        KeyO => return letter('o'),
        KeyP => return letter('p'),
        KeyQ => return letter('q'),
        KeyR => return letter('r'),
        KeyS => return letter('s'),
        KeyT => return letter('t'),
        KeyU => return letter('u'),
        KeyV => return letter('v'),
        KeyW => return letter('w'),
        KeyX => return letter('x'),
        KeyY => return letter('y'),
        KeyZ => return letter('z'),
        Digit0 => "0".into(),
        Digit1 => "1".into(),
        Digit2 => "2".into(),
        Digit3 => "3".into(),
        Digit4 => "4".into(),
        Digit5 => "5".into(),
        Digit6 => "6".into(),
        Digit7 => "7".into(),
        Digit8 => "8".into(),
        Digit9 => "9".into(),
        F1 => "f1".into(),
        F2 => "f2".into(),
        F3 => "f3".into(),
        F4 => "f4".into(),
        F5 => "f5".into(),
        F6 => "f6".into(),
        F7 => "f7".into(),
        F8 => "f8".into(),
        F9 => "f9".into(),
        F10 => "f10".into(),
        F11 => "f11".into(),
        F12 => "f12".into(),
        // Everything else -- punctuation, arrows, editing keys -- is better
        // identified by the character the layout produced, exactly as the
        // browser build prefers ev.key for symbols.
        _ => return None,
    })
}

/// Records the physical key of every press, then lets the event through.
pub struct PhysicalKeyRecorder;

impl CustomApplicationHandler for PhysicalKeyRecorder {
    fn window_event(
        &mut self,
        _event_loop: &slint::winit_030::winit::event_loop::ActiveEventLoop,
        _window_id: slint::winit_030::winit::window::WindowId,
        _winit_window: Option<&slint::winit_030::winit::window::Window>,
        _slint_window: Option<&slint::Window>,
        event: &WindowEvent,
    ) -> EventResult {
        if let WindowEvent::KeyboardInput { event, .. } = event {
            if event.state == ElementState::Pressed {
                let physical = match event.physical_key {
                    PhysicalKey::Code(code) => name(code),
                    PhysicalKey::Unidentified(_) => None,
                };
                LAST.with(|c| *c.borrow_mut() = physical);
            }
        }
        // Slint must still see the event; we only observe.
        EventResult::Propagate
    }
}
