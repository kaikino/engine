//! The cartridge interface.

use crate::apu::Apu;
use crate::gpu::Gpu;
use crate::hash::StateHasher;
use crate::input::Input;

/// Maximum number of simultaneous players a cartridge can support.
pub const MAX_PLAYERS: usize = 4;

/// Simulation and display rate in frames per second.
pub const TICK_RATE: u32 = 60;

/// Everything a cartridge may read or trigger during one simulation step.
///
/// Sound is triggered from `update`, not `draw`, so it is part of the
/// deterministic timeline and replays sound identical.
pub struct UpdateCtx<'a> {
    /// Controller state for every player slot this frame. Unused slots are
    /// [`Input::NONE`].
    pub inputs: [Input; MAX_PLAYERS],
    /// Frame number, starting at zero.
    pub frame: u32,
    /// Number of active players (`1..=MAX_PLAYERS`).
    pub players: u8,
    /// The sound processor, for triggering effects and music.
    pub apu: &'a mut Apu,
}

impl<'a> UpdateCtx<'a> {
    /// Builds a context for one frame.
    pub fn new(
        inputs: [Input; MAX_PLAYERS],
        frame: u32,
        players: u8,
        apu: &'a mut Apu,
    ) -> UpdateCtx<'a> {
        UpdateCtx {
            inputs,
            frame,
            players,
            apu,
        }
    }

    /// Input for player `p` (0-based). Out-of-range players read as no input.
    pub fn input(&self, p: usize) -> Input {
        self.inputs.get(p).copied().unwrap_or(Input::NONE)
    }
}

/// A KAGAMI cartridge.
///
/// The three rules of determinism:
///
/// 1. `update` must depend only on `self` and the [`UpdateCtx`] it is given.
///    No clocks, no floats, no global state, no iteration over hash maps.
/// 2. `draw` must not mutate game state (it takes `&self`, so the compiler
///    enforces this).
/// 3. `hash` must cover every field that influences `update`. If a field is
///    missed, a desync in that field will go undetected.
pub trait Game {
    /// Creates a fresh game from a match seed. All players call this with the
    /// same seed.
    fn new(seed: u32) -> Self
    where
        Self: Sized;

    /// Advances the simulation by exactly one frame.
    fn update(&mut self, ctx: &mut UpdateCtx);

    /// Draws the current state. May be called any number of times (or not at
    /// all) between updates.
    fn draw(&self, gpu: &mut Gpu);

    /// Feeds all simulation-relevant state into the hasher.
    fn hash(&self, h: &mut StateHasher);

    /// Title shown in the window chrome. Defaults to the type name.
    fn title(&self) -> &str {
        core::any::type_name::<Self>()
    }

    /// Number of players the cartridge expects by default.
    fn default_players(&self) -> u8 {
        1
    }
}

/// Runs a game headlessly for `frames` frames, feeding `inputs(frame)` each
/// step, and returns the final state hash. The workhorse of determinism tests.
pub fn simulate<G: Game>(
    game: &mut G,
    apu: &mut Apu,
    frames: u32,
    players: u8,
    mut inputs: impl FnMut(u32) -> [Input; MAX_PLAYERS],
) -> u64 {
    for frame in 0..frames {
        let mut ctx = UpdateCtx::new(inputs(frame), frame, players, apu);
        game.update(&mut ctx);
    }
    let mut h = StateHasher::new();
    game.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Counter {
        n: i32,
    }
    impl Game for Counter {
        fn new(seed: u32) -> Self {
            Counter { n: seed as i32 }
        }
        fn update(&mut self, ctx: &mut UpdateCtx) {
            self.n += ctx.input(0).axis_x() + ctx.input(9).axis_x();
            if ctx.frame == 3 {
                ctx.apu.sfx(0, &crate::apu::Sfx::blip());
            }
        }
        fn draw(&self, _gpu: &mut Gpu) {}
        fn hash(&self, h: &mut StateHasher) {
            h.write_i32(self.n);
        }
    }

    #[test]
    fn simulate_is_deterministic() {
        let mut apu = Apu::new();
        let mut a = Counter::new(1);
        let mut b = Counter::new(1);
        let inputs = |f: u32| {
            let mut i = [Input::NONE; MAX_PLAYERS];
            i[0] = if f % 2 == 0 {
                Input::RIGHT
            } else {
                Input::NONE
            };
            i
        };
        let ha = simulate(&mut a, &mut apu, 10, 2, inputs);
        let hb = simulate(&mut b, &mut apu, 10, 2, inputs);
        assert_eq!(ha, hb);
        assert_eq!(a.n, 6);
        assert!(a.title().contains("Counter"));
        assert_eq!(a.default_players(), 1);
    }

    #[test]
    fn ctx_helpers() {
        let mut apu = Apu::new();
        let ctx = UpdateCtx::new([Input::A; MAX_PLAYERS], 5, 2, &mut apu);
        assert_eq!(ctx.input(0), Input::A);
        assert_eq!(ctx.input(99), Input::NONE);
        assert_eq!(ctx.frame, 5);
    }
}
