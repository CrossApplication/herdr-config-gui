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
//!
//! The same handler also records the modifier state, for a second reason.
//! Slint's winit backend swaps Control and Command on Apple platforms:
//!
//! ```text
//! // For now: Match Qt's behavior of mapping command to control and control to meta (LWin/RWin).
//! let swap_cmd_ctrl = i_slint_core::is_apple_platform();
//! ```
//!
//! That is a sensible default for an application whose own shortcuts should
//! read `Ctrl+C` everywhere. It is wrong here: what gets written to
//! config.toml has to be the modifier the *terminal* will receive, not the
//! one a cross-platform convention renames it to. Left alone, a user pressing
//! ctrl+a would have `cmd+a` saved, and the binding would never fire.
//! winit reports the modifiers before that swap, so they are taken from here.

use std::cell::RefCell;

use slint::winit_030::{
    winit::{
        event::{ElementState, WindowEvent},
        keyboard::{KeyCode, ModifiersState, PhysicalKey},
    },
    CustomApplicationHandler, EventResult,
};

/// The modifiers as winit reports them, before Slint's Apple swap.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

impl From<ModifiersState> for Modifiers {
    fn from(s: ModifiersState) -> Self {
        Self {
            ctrl: s.control_key(),
            shift: s.shift_key(),
            alt: s.alt_key(),
            meta: s.super_key(),
        }
    }
}

thread_local! {
    /// The physical key of the most recent press, in `ev.code` terms.
    static LAST: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Every physical key seen, verbatim, for diagnosing remappers that
    /// rewrite events before any application sees them.
    static LAST_RAW: RefCell<String> = const { RefCell::new(String::new()) };
    /// The modifier state winit last reported, unswapped.
    static MODS: RefCell<Modifiers> = const { RefCell::new(Modifiers {
        ctrl: false, shift: false, alt: false, meta: false,
    }) };
}

/// The modifiers held right now, as winit sees them.
pub fn modifiers() -> Modifiers {
    MODS.with(|c| *c.borrow())
}

/// The untranslated `PhysicalKey` of the most recent press.
pub fn last_raw() -> String {
    LAST_RAW.with(|c| c.borrow().clone())
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
        if let WindowEvent::ModifiersChanged(mods) = event {
            MODS.with(|c| *c.borrow_mut() = mods.state().into());
        }
        if let WindowEvent::KeyboardInput { event, .. } = event {
            if event.state == ElementState::Pressed {
                let physical = match event.physical_key {
                    PhysicalKey::Code(code) => name(code),
                    PhysicalKey::Unidentified(_) => None,
                };
                // Both halves of what winit reports, so a disagreement
                // between the position and the meaning is visible.
                LAST_RAW.with(|c| {
                    *c.borrow_mut() = format!(
                        "physical={:?} logical={:?} text={:?}",
                        event.physical_key, event.logical_key, event.text
                    )
                });
                LAST.with(|c| *c.borrow_mut() = physical);
            }
        }
        // Slint must still see the event; we only observe.
        EventResult::Propagate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Slint's winit backend swaps Control and Command on Apple platforms, so
    /// a binding captured through Slint's own modifiers would name the wrong
    /// key. These come from winit directly and must not swap.
    #[test]
    fn modifiers_are_taken_unswapped() {
        let m: Modifiers = ModifiersState::CONTROL.into();
        assert!(m.ctrl, "control must stay control");
        assert!(!m.meta, "control must not become command");

        let m: Modifiers = ModifiersState::SUPER.into();
        assert!(m.meta, "command must stay command");
        assert!(!m.ctrl, "command must not become control");
    }

    #[test]
    fn every_modifier_survives_the_conversion() {
        let all = ModifiersState::CONTROL
            | ModifiersState::SHIFT
            | ModifiersState::ALT
            | ModifiersState::SUPER;
        let m: Modifiers = all.into();
        assert_eq!(m, Modifiers { ctrl: true, shift: true, alt: true, meta: true });
        assert_eq!(Modifiers::from(ModifiersState::empty()), Modifiers::default());
    }

    #[test]
    fn letters_digits_and_function_keys_are_named_the_way_ev_code_names_them() {
        assert_eq!(name(KeyCode::KeyA).as_deref(), Some("a"));
        assert_eq!(name(KeyCode::Digit7).as_deref(), Some("7"));
        assert_eq!(name(KeyCode::F12).as_deref(), Some("f12"));
        // Punctuation is better identified by the character the layout made.
        assert_eq!(name(KeyCode::Minus), None);
        assert_eq!(name(KeyCode::ArrowLeft), None);
    }
}
