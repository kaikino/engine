//! Keyboard to controller mapping.

use kagami::{Input, MAX_PLAYERS};
use winit::keyboard::KeyCode;

/// Maps physical keys to player inputs. Two local players share one keyboard.
#[derive(Clone, Debug)]
pub struct KeyMap {
    bindings: Vec<(KeyCode, u8, Input)>,
}

impl Default for KeyMap {
    fn default() -> Self {
        use KeyCode as K;
        KeyMap {
            bindings: vec![
                (K::ArrowUp, 0, Input::UP),
                (K::ArrowDown, 0, Input::DOWN),
                (K::ArrowLeft, 0, Input::LEFT),
                (K::ArrowRight, 0, Input::RIGHT),
                (K::KeyZ, 0, Input::A),
                (K::KeyX, 0, Input::B),
                (K::KeyC, 0, Input::C),
                (K::Enter, 0, Input::START),
                (K::KeyW, 1, Input::UP),
                (K::KeyS, 1, Input::DOWN),
                (K::KeyA, 1, Input::LEFT),
                (K::KeyD, 1, Input::RIGHT),
                (K::KeyJ, 1, Input::A),
                (K::KeyK, 1, Input::B),
                (K::KeyL, 1, Input::C),
                (K::Tab, 1, Input::START),
            ],
        }
    }
}

impl KeyMap {
    /// Looks up which player and button a key drives.
    pub fn lookup(&self, key: KeyCode) -> Option<(u8, Input)> {
        self.bindings
            .iter()
            .find(|(k, _, _)| *k == key)
            .map(|(_, p, i)| (*p, *i))
    }
}

/// Live controller state for every local player.
#[derive(Clone, Debug, Default)]
pub struct Controllers {
    state: [Input; MAX_PLAYERS],
}

impl Controllers {
    /// Applies a key press or release.
    pub fn apply(&mut self, map: &KeyMap, key: KeyCode, pressed: bool) {
        if let Some((player, button)) = map.lookup(key) {
            let slot = &mut self.state[player as usize % MAX_PLAYERS];
            if pressed {
                *slot |= button;
            } else {
                *slot = *slot & !button;
            }
        }
    }

    /// Current input for a player.
    pub fn input(&self, player: usize) -> Input {
        self.state.get(player).copied().unwrap_or(Input::NONE)
    }

    /// All players' inputs.
    pub fn all(&self) -> [Input; MAX_PLAYERS] {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_and_state() {
        let map = KeyMap::default();
        assert_eq!(map.lookup(KeyCode::KeyZ), Some((0, Input::A)));
        assert_eq!(map.lookup(KeyCode::KeyJ), Some((1, Input::A)));
        assert_eq!(map.lookup(KeyCode::F12), None);
        let mut c = Controllers::default();
        c.apply(&map, KeyCode::ArrowLeft, true);
        c.apply(&map, KeyCode::KeyZ, true);
        c.apply(&map, KeyCode::KeyW, true);
        c.apply(&map, KeyCode::F12, true);
        assert_eq!(c.input(0), Input::LEFT | Input::A);
        assert_eq!(c.input(1), Input::UP);
        assert_eq!(c.input(7), Input::NONE);
        c.apply(&map, KeyCode::KeyZ, false);
        assert_eq!(c.input(0), Input::LEFT);
        assert_eq!(c.all()[1], Input::UP);
    }
}
