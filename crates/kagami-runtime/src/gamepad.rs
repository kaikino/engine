//! Gamepads, mapped onto the KAGAMI controller in connection order.

use kagami::{Input, MAX_PLAYERS};

/// Stick deflection (0..1) past which an axis counts as a d-pad press.
pub const DEADZONE: f32 = 0.5;

/// Maps a standard gamepad button to a controller button.
#[cfg(feature = "gamepad")]
pub fn map_button(b: gilrs::Button) -> Option<Input> {
    use gilrs::Button as B;
    Some(match b {
        B::South => Input::A,
        B::East => Input::B,
        B::West | B::North => Input::C,
        B::Start | B::Select => Input::START,
        B::DPadUp => Input::UP,
        B::DPadDown => Input::DOWN,
        B::DPadLeft => Input::LEFT,
        B::DPadRight => Input::RIGHT,
        _ => return None,
    })
}

/// Converts a stick axis reading into d-pad bits: `(negative, positive)`
/// buttons for that axis and which (if either) is pressed.
pub fn axis_to_dpad(horizontal: bool, value: f32) -> (Input, Input, Option<Input>) {
    let (neg, pos) = if horizontal {
        (Input::LEFT, Input::RIGHT)
    } else {
        (Input::UP, Input::DOWN)
    };
    let pressed = if value <= -DEADZONE {
        Some(neg)
    } else if value >= DEADZONE {
        Some(pos)
    } else {
        None
    };
    (neg, pos, pressed)
}

/// Per-player gamepad state, kept separately from the keyboard so the two
/// can be OR-ed together.
#[derive(Clone, Debug, Default)]
pub struct PadState {
    buttons: [Input; MAX_PLAYERS],
    sticks: [Input; MAX_PLAYERS],
}

impl PadState {
    /// Applies a button press or release for a player.
    pub fn button(&mut self, player: usize, b: Input, pressed: bool) {
        if player >= MAX_PLAYERS {
            return;
        }
        if pressed {
            self.buttons[player] |= b;
        } else {
            self.buttons[player] = self.buttons[player] & !b;
        }
    }

    /// Applies a stick axis reading for a player.
    pub fn axis(&mut self, player: usize, horizontal: bool, value: f32) {
        if player >= MAX_PLAYERS {
            return;
        }
        let (neg, pos, pressed) = axis_to_dpad(horizontal, value);
        self.sticks[player] = self.sticks[player] & !(neg | pos);
        if let Some(p) = pressed {
            self.sticks[player] |= p;
        }
    }

    /// Combined input for a player.
    pub fn input(&self, player: usize) -> Input {
        if player >= MAX_PLAYERS {
            return Input::NONE;
        }
        self.buttons[player] | self.sticks[player]
    }
}

/// Polls connected gamepads through `gilrs`.
#[cfg(feature = "gamepad")]
pub struct Gamepads {
    gilrs: gilrs::Gilrs,
    order: Vec<gilrs::GamepadId>,
    state: PadState,
}

#[cfg(feature = "gamepad")]
impl Gamepads {
    /// Opens the gamepad subsystem. Returns `None` if it is unavailable.
    pub fn open() -> Option<Gamepads> {
        let gilrs = gilrs::Gilrs::new().ok()?;
        let mut order: Vec<gilrs::GamepadId> = gilrs.gamepads().map(|(id, _)| id).collect();
        order.sort_by_key(|&id| usize::from(id));
        Some(Gamepads {
            gilrs,
            order,
            state: PadState::default(),
        })
    }

    fn player_of(&mut self, id: gilrs::GamepadId) -> usize {
        if let Some(i) = self.order.iter().position(|&g| g == id) {
            return i;
        }
        self.order.push(id);
        self.order.len() - 1
    }

    /// Drains pending events into the pad state.
    pub fn poll(&mut self) {
        use gilrs::{Axis, EventType};
        while let Some(ev) = self.gilrs.next_event() {
            let player = self.player_of(ev.id);
            match ev.event {
                EventType::ButtonPressed(b, _) => {
                    if let Some(i) = map_button(b) {
                        self.state.button(player, i, true);
                    }
                }
                EventType::ButtonReleased(b, _) => {
                    if let Some(i) = map_button(b) {
                        self.state.button(player, i, false);
                    }
                }
                EventType::AxisChanged(Axis::LeftStickX, v, _) => self.state.axis(player, true, v),
                // gilrs reports Y up-positive; the console's d-pad is down-positive.
                EventType::AxisChanged(Axis::LeftStickY, v, _) => {
                    self.state.axis(player, false, -v);
                }
                EventType::Disconnected => {
                    self.state.button(player, Input::from_bits(0xFF), false);
                    self.state.axis(player, true, 0.0);
                    self.state.axis(player, false, 0.0);
                }
                _ => {}
            }
        }
    }

    /// Input for a player from their gamepad.
    pub fn input(&self, player: usize) -> Input {
        self.state.input(player)
    }

    /// Number of gamepads seen so far.
    pub fn count(&self) -> usize {
        self.order.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_thresholds() {
        assert_eq!(axis_to_dpad(true, -1.0).2, Some(Input::LEFT));
        assert_eq!(axis_to_dpad(true, 0.9).2, Some(Input::RIGHT));
        assert_eq!(axis_to_dpad(true, 0.2).2, None);
        assert_eq!(axis_to_dpad(false, -0.7).2, Some(Input::UP));
        assert_eq!(axis_to_dpad(false, 0.7).2, Some(Input::DOWN));
    }

    #[test]
    fn pad_state_merges_buttons_and_sticks() {
        let mut p = PadState::default();
        p.button(0, Input::A, true);
        p.axis(0, true, -1.0);
        assert_eq!(p.input(0), Input::A | Input::LEFT);
        p.axis(0, true, 0.0);
        assert_eq!(p.input(0), Input::A);
        p.axis(0, true, 1.0);
        p.button(0, Input::A, false);
        assert_eq!(p.input(0), Input::RIGHT);
        p.button(9, Input::A, true);
        p.axis(9, true, 1.0);
        assert_eq!(p.input(9), Input::NONE);
        assert_eq!(p.input(1), Input::NONE);
    }

    #[cfg(feature = "gamepad")]
    #[test]
    fn button_map() {
        use gilrs::Button as B;
        assert_eq!(map_button(B::South), Some(Input::A));
        assert_eq!(map_button(B::East), Some(Input::B));
        assert_eq!(map_button(B::West), Some(Input::C));
        assert_eq!(map_button(B::Start), Some(Input::START));
        assert_eq!(map_button(B::DPadLeft), Some(Input::LEFT));
        assert_eq!(map_button(B::LeftTrigger), None);
        // Opening the subsystem must never panic, with or without devices.
        if let Some(mut pads) = Gamepads::open() {
            pads.poll();
            let _ = pads.count();
            assert_eq!(pads.input(3), Input::NONE);
        }
    }
}
