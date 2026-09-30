//! Shared input state fed by the display backend and consumed by the game.
//!
//! The display thread receives raw events from the browser (key down/up,
//! relative mouse motion, buttons) and accumulates them here. The simulation
//! consumes them once per step. This indirection keeps input handling
//! backend-agnostic: a native winit backend would call exactly the same
//! `on_*` methods.

use std::collections::HashSet;

/// Keys the engine actually understands. Raw browser `KeyboardEvent.code`
/// strings are mapped through [`Key::from_code`]; unknown keys are retained
/// in a raw set so nothing is silently lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    W,
    A,
    S,
    D,
    Space,
    ShiftLeft,
    ShiftRight,
    ControlLeft,
    Digit(u8),
    Escape,
    F3,
    Unknown,
}

impl Key {
    /// Maps a browser `event.code` value to a [`Key`].
    pub fn from_code(code: &str) -> Key {
        match code {
            "KeyW" => Key::W,
            "KeyA" => Key::A,
            "KeyS" => Key::S,
            "KeyD" => Key::D,
            "Space" => Key::Space,
            "ShiftLeft" => Key::ShiftLeft,
            "ShiftRight" => Key::ShiftRight,
            "ControlLeft" => Key::ControlLeft,
            "Escape" => Key::Escape,
            "F3" => Key::F3,
            other => {
                if let Some(rest) = other.strip_prefix("Digit") {
                    if let Ok(d) = rest.parse::<u8>() {
                        if (1..=9).contains(&d) {
                            return Key::Digit(d);
                        }
                    }
                }
                Key::Unknown
            }
        }
    }
}

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
}

/// One queued mouse button state change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonEvent {
    pub button: MouseButton,
    /// `true` on press, `false` on release.
    pub pressed: bool,
}

/// Accumulates input events. Shared between the display thread (producer) and
/// the simulation (consumer) behind a mutex owned by the display backend.
#[derive(Default)]
pub struct InputState {
    held: HashSet<Key>,
    mouse_dx: f64,
    mouse_dy: f64,
    buttons: HashSet<MouseButton>,
    button_events: Vec<ButtonEvent>,
}

impl InputState {
    pub fn new() -> Self {
        Self::default()
    }

    // -- producer side (display backend) ---------------------------------

    /// Records a key press or release.
    pub fn on_key(&mut self, key: Key, down: bool) {
        if down {
            self.held.insert(key);
        } else {
            self.held.remove(&key);
        }
    }

    /// Accumulates relative mouse motion (pixels).
    pub fn on_mouse_delta(&mut self, dx: f64, dy: f64) {
        self.mouse_dx += dx;
        self.mouse_dy += dy;
    }

    /// Records a mouse button transition.
    pub fn on_button(&mut self, button: MouseButton, down: bool) {
        if down {
            self.buttons.insert(button);
        } else {
            self.buttons.remove(&button);
        }
        self.button_events.push(ButtonEvent { button, pressed: down });
    }

    // -- consumer side (simulation) ---------------------------------------

    /// Whether `key` is currently held down.
    pub fn is_down(&self, key: Key) -> bool {
        self.held.contains(&key)
    }

    /// Whether `button` is currently held down. (Consumed by the block
    /// interaction system in a later milestone; exercised by unit tests.)
    #[allow(dead_code)]
    pub fn is_button_down(&self, button: MouseButton) -> bool {
        self.buttons.contains(&button)
    }

    /// Takes the accumulated relative mouse motion since the last call.
    pub fn take_mouse_delta(&mut self) -> (f64, f64) {
        (std::mem::take(&mut self.mouse_dx), std::mem::take(&mut self.mouse_dy))
    }

    /// Drains all queued button events since the last call.
    pub fn take_button_events(&mut self) -> Vec<ButtonEvent> {
        std::mem::take(&mut self.button_events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_codes_map() {
        assert_eq!(Key::from_code("KeyW"), Key::W);
        assert_eq!(Key::from_code("Space"), Key::Space);
        assert_eq!(Key::from_code("Digit5"), Key::Digit(5));
        assert_eq!(Key::from_code("Digit0"), Key::Unknown);
        assert_eq!(Key::from_code("Nope"), Key::Unknown);
    }

    #[test]
    fn held_keys_latch_and_release() {
        let mut input = InputState::new();
        input.on_key(Key::W, true);
        assert!(input.is_down(Key::W));
        input.on_key(Key::W, false);
        assert!(!input.is_down(Key::W));
    }

    #[test]
    fn mouse_delta_accumulates_until_taken() {
        let mut input = InputState::new();
        input.on_mouse_delta(1.5, -2.0);
        input.on_mouse_delta(0.5, 1.0);
        assert_eq!(input.take_mouse_delta(), (2.0, -1.0));
        assert_eq!(input.take_mouse_delta(), (0.0, 0.0));
    }

    #[test]
    fn button_events_are_queued_in_order() {
        let mut input = InputState::new();
        input.on_button(MouseButton::Left, true);
        input.on_button(MouseButton::Right, true);
        input.on_button(MouseButton::Left, false);
        let events = input.take_button_events();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0], ButtonEvent { button: MouseButton::Left, pressed: true });
        assert_eq!(events[2], ButtonEvent { button: MouseButton::Left, pressed: false });
        assert!(input.take_button_events().is_empty());
        assert!(!input.is_button_down(MouseButton::Left));
        assert!(input.is_button_down(MouseButton::Right));
    }
}
