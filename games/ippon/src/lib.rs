//! IPPON (一本): a two-player kendo duel and KAGAMI's reference cartridge.
//!
//! Two kendoka face off in a dojo. Close the distance, wind up, strike *men*
//! before your opponent blocks. Sanbon-shōbu rules: two ippon win the match.
//!
//! The whole game is deterministic fixed-point simulation, so it runs in
//! lockstep across the network and its matches replay from a few kilobytes
//! of input. With one player, the second kendoka is driven by a small CPU
//! opponent fed from the match seed.
//!
//! Controls: left/right move, A strikes, B blocks, START begins.
#![deny(clippy::float_arithmetic)]

pub mod art;
pub mod music;

use kagami::gpu::{font, Sprite, PRIORITY, SCREEN_W};
use kagami::input::Edge;
use kagami::{palette as pal, Fx, Game, Gpu, Input, Rng, StateHasher, UpdateCtx};

const FLOOR_Y: i32 = 168;
const LEFT_BOUND: Fx = Fx::from_int(20);
const RIGHT_BOUND: Fx = Fx::from_int(236);
const MIN_GAP: Fx = Fx::from_int(14);
const WALK_SPEED: Fx = Fx::from_ratio(5, 4);
const START_X: [i32; 2] = [80, 176];

const WINDUP_FRAMES: u32 = 8;
const STRIKE_FRAMES: u32 = 5;
const RECOVER_FRAMES: u32 = 14;
const BLOCKED_RECOVER_FRAMES: u32 = 26;
const HAJIME_FRAMES: u32 = 70;
const POINT_FRAMES: u32 = 110;
const IPPON_TO_WIN: u8 = 2;
const PETALS: usize = 12;

/// What a kendoka is doing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stance {
    /// Standing in chūdan.
    Idle,
    /// Stepping.
    Walk,
    /// Shinai raised, about to strike.
    Windup,
    /// Strike is active and can land.
    Strike,
    /// Recovering after a strike.
    Recover,
    /// Guarding; blocks strikes from the front.
    Block,
    /// Just struck.
    Hit,
}

impl Stance {
    fn tag(self) -> u8 {
        match self {
            Stance::Idle => 0,
            Stance::Walk => 1,
            Stance::Windup => 2,
            Stance::Strike => 3,
            Stance::Recover => 4,
            Stance::Block => 5,
            Stance::Hit => 6,
        }
    }
}

/// One kendoka.
#[derive(Clone, Copy, Debug)]
pub struct Fighter {
    /// Horizontal center.
    pub x: Fx,
    /// `1` faces right, `-1` faces left.
    pub facing: i32,
    /// Current stance.
    pub stance: Stance,
    /// Frames spent in the current stance.
    pub timer: u32,
    /// Walk animation counter.
    pub anim: u32,
    /// Points scored.
    pub ippon: u8,
    /// Whether the CPU drives this fighter.
    pub cpu: bool,
    edge: Edge,
    cpu_block: u32,
}

impl Fighter {
    fn new(x: i32, facing: i32, cpu: bool) -> Fighter {
        Fighter {
            x: Fx::from_int(x),
            facing,
            stance: Stance::Idle,
            timer: 0,
            anim: 0,
            ippon: 0,
            cpu,
            edge: Edge::default(),
            cpu_block: 0,
        }
    }

    fn reset_position(&mut self, x: i32, facing: i32) {
        self.x = Fx::from_int(x);
        self.facing = facing;
        self.stance = Stance::Idle;
        self.timer = 0;
        self.anim = 0;
        self.cpu_block = 0;
    }

    fn set(&mut self, s: Stance) {
        self.stance = s;
        self.timer = 0;
    }

    fn busy(&self) -> bool {
        matches!(
            self.stance,
            Stance::Windup | Stance::Strike | Stance::Recover | Stance::Hit
        )
    }

    /// Where the shinai can land this frame, as `(near, far)` x range.
    fn strike_reach(&self) -> (Fx, Fx) {
        let near = self.x + Fx::from_int(8 * self.facing);
        let far = self.x + Fx::from_int(30 * self.facing);
        (near.min(far), near.max(far))
    }

    fn hurt_box(&self) -> (Fx, Fx) {
        (self.x - Fx::from_int(6), self.x + Fx::from_int(6))
    }

    fn hash(&self, h: &mut StateHasher) {
        h.write_fx(self.x);
        h.write_i32(self.facing);
        h.write_u8(self.stance.tag());
        h.write_u32(self.timer);
        h.write_u32(self.anim);
        h.write_u8(self.ippon);
        h.write_bool(self.cpu);
        h.write_u8(self.edge.current().bits());
        h.write_u32(self.cpu_block);
    }
}

/// Match flow.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Title screen; waiting for START.
    Title,
    /// "Hajime!" countdown before fighting.
    Hajime(u32),
    /// Live play.
    Fight,
    /// A point was scored (`winner` 0 or 1) or both struck at once (`2`).
    Point {
        /// Who scored, or 2 for ai-uchi.
        winner: u8,
        /// Frames remaining in the freeze.
        t: u32,
    },
    /// Match over.
    Victory {
        /// Winning player, 0 or 1.
        winner: u8,
    },
}

#[derive(Clone, Copy, Debug)]
struct Petal {
    x: Fx,
    y: Fx,
    vy: Fx,
    phase: Fx,
}

/// The game.
#[derive(Clone, Debug)]
pub struct Ippon {
    fighters: [Fighter; 2],
    phase: Phase,
    petals: [Petal; PETALS],
    rng: Rng,
    frame: u32,
    players: u8,
}

impl Ippon {
    /// Current scores.
    pub fn score(&self) -> [u8; 2] {
        [self.fighters[0].ippon, self.fighters[1].ippon]
    }

    /// Current match phase.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Both kendoka.
    pub fn fighters(&self) -> &[Fighter; 2] {
        &self.fighters
    }

    fn spawn_petal(rng: &mut Rng, from_top: bool) -> Petal {
        Petal {
            x: Fx::from_int(rng.range(0, SCREEN_W as i32 - 1)),
            y: if from_top {
                Fx::from_int(-8)
            } else {
                Fx::from_int(rng.range(0, 200))
            },
            vy: rng.fx_range(Fx::from_ratio(1, 4), Fx::from_ratio(3, 4)),
            phase: rng.fx(),
        }
    }

    fn cpu_input(&mut self, me: usize) -> Input {
        let other = 1 - me;
        let (f, o) = (self.fighters[me], self.fighters[other]);
        let dx = o.x - f.x;
        let dist = dx.abs();
        let toward = if dx.is_negative() {
            Input::LEFT
        } else {
            Input::RIGHT
        };
        let away = if dx.is_negative() {
            Input::RIGHT
        } else {
            Input::LEFT
        };
        if self.fighters[me].cpu_block > 0 {
            self.fighters[me].cpu_block -= 1;
            return Input::B;
        }
        if f.busy() {
            return Input::NONE;
        }
        if o.stance == Stance::Windup && self.rng.chance(2, 5) {
            self.fighters[me].cpu_block = 12 + self.rng.below(10);
            return Input::B;
        }
        if dist > Fx::from_int(36) {
            return if self.rng.chance(9, 10) {
                toward
            } else {
                Input::NONE
            };
        }
        if dist < Fx::from_int(20) && self.rng.chance(1, 2) {
            return away;
        }
        if self.rng.chance(1, 18) {
            return Input::A;
        }
        if self.rng.chance(1, 50) {
            self.fighters[me].cpu_block = 8 + self.rng.below(8);
            return Input::B;
        }
        Input::NONE
    }

    fn fighter_update(&mut self, i: usize, input: Input, ctx: &mut UpdateCtx) {
        let other = 1 - i;
        let other_x = self.fighters[other].x;
        let f = &mut self.fighters[i];
        f.edge.update(input);
        f.timer += 1;
        match f.stance {
            Stance::Windup => {
                if f.timer >= WINDUP_FRAMES {
                    f.set(Stance::Strike);
                    ctx.apu.sfx(1 + i, &music::swing());
                }
            }
            Stance::Strike => {
                if f.timer >= STRIKE_FRAMES {
                    f.set(Stance::Recover);
                }
            }
            Stance::Recover => {
                let needed = if f.anim == u32::MAX {
                    BLOCKED_RECOVER_FRAMES
                } else {
                    RECOVER_FRAMES
                };
                if f.timer >= needed {
                    f.anim = 0;
                    f.set(Stance::Idle);
                }
            }
            Stance::Hit => {}
            Stance::Idle | Stance::Walk | Stance::Block => {
                if f.edge.pressed(Input::A) {
                    f.set(Stance::Windup);
                } else if f.edge.held(Input::B) {
                    if f.stance != Stance::Block {
                        f.set(Stance::Block);
                    }
                } else {
                    let ax = input.axis_x();
                    if ax != 0 {
                        let nx = f.x + WALK_SPEED * ax;
                        // Keep inside the dojo and never overlap the opponent.
                        let (lo, hi) = if other_x > f.x {
                            (LEFT_BOUND, (other_x - MIN_GAP).max(LEFT_BOUND))
                        } else {
                            ((other_x + MIN_GAP).min(RIGHT_BOUND), RIGHT_BOUND)
                        };
                        f.x = nx.clamp(lo, hi);
                        if f.stance != Stance::Walk {
                            f.set(Stance::Walk);
                        }
                        f.anim += 1;
                        if f.anim % 16 == 1 {
                            ctx.apu.sfx(3, &music::step());
                        }
                    } else if f.stance != Stance::Idle {
                        f.set(Stance::Idle);
                    }
                }
                // Always face the opponent.
                f.facing = if other_x > f.x { 1 } else { -1 };
            }
        }
    }

    fn resolve_strikes(&mut self, ctx: &mut UpdateCtx) {
        let lands = |a: &Fighter, d: &Fighter| -> bool {
            if a.stance != Stance::Strike || d.stance == Stance::Hit {
                return false;
            }
            let (near, far) = a.strike_reach();
            let (lo, hi) = d.hurt_box();
            near <= hi && far >= lo
        };
        let a_hits_b = lands(&self.fighters[0], &self.fighters[1]);
        let b_hits_a = lands(&self.fighters[1], &self.fighters[0]);
        if !a_hits_b && !b_hits_a {
            return;
        }
        if a_hits_b && b_hits_a {
            // Ai-uchi: simultaneous strikes cancel.
            for f in &mut self.fighters {
                f.anim = u32::MAX;
                f.set(Stance::Recover);
            }
            ctx.apu.sfx(0, &music::block());
            self.phase = Phase::Point {
                winner: 2,
                t: POINT_FRAMES / 2,
            };
            return;
        }
        let (att, def) = if a_hits_b { (0, 1) } else { (1, 0) };
        let defender = self.fighters[def];
        let blocked =
            defender.stance == Stance::Block && defender.facing != self.fighters[att].facing;
        if blocked {
            self.fighters[att].anim = u32::MAX;
            self.fighters[att].set(Stance::Recover);
            ctx.apu.sfx(0, &music::block());
            return;
        }
        self.fighters[att].ippon += 1;
        self.fighters[att].set(Stance::Recover);
        self.fighters[def].set(Stance::Hit);
        ctx.apu.stop_song();
        ctx.apu.sfx(0, &music::hit());
        ctx.apu.sfx(1, &music::ippon_call());
        self.phase = Phase::Point {
            winner: att as u8,
            t: POINT_FRAMES,
        };
    }

    fn reset_round(&mut self) {
        self.fighters[0].reset_position(START_X[0], 1);
        self.fighters[1].reset_position(START_X[1], -1);
    }

    fn update_petals(&mut self) {
        for i in 0..PETALS {
            let mut p = self.petals[i];
            p.y += p.vy;
            p.phase += Fx::from_ratio(1, 90);
            p.x += Fx::sin_turns(p.phase) * Fx::from_ratio(3, 8);
            if p.y > Fx::from_int(224) {
                p = Ippon::spawn_petal(&mut self.rng, true);
            }
            if p.x < Fx::from_int(-8) {
                p.x = Fx::from_int(SCREEN_W as i32);
            } else if p.x > Fx::from_int(SCREEN_W as i32) {
                p.x = Fx::from_int(-8);
            }
            self.petals[i] = p;
        }
    }

    fn draw_fighter(&self, gpu: &mut Gpu, i: usize) {
        let f = &self.fighters[i];
        let offset = if i == 1 { art::pose::P2_OFFSET } else { 0 };
        let (tile, wide) = match f.stance {
            Stance::Idle | Stance::Recover => (art::pose::IDLE, false),
            Stance::Walk => (
                if (f.anim / 8) % 2 == 0 {
                    art::pose::WALK1
                } else {
                    art::pose::WALK2
                },
                false,
            ),
            Stance::Windup => (art::pose::WINDUP, false),
            Stance::Strike => (art::pose::STRIKE, true),
            Stance::Block => (art::pose::BLOCK, false),
            Stance::Hit => (art::pose::HIT, false),
        };
        let flip = f.facing < 0;
        let x = f.x.round();
        let left = if wide && flip { x - 16 } else { x - 8 };
        let w = if wide { 3 } else { 2 };
        gpu.sprite(
            Sprite::new(left, FLOOR_Y - 24, tile + offset)
                .size(w, 3)
                .flip_x(flip),
        );
    }

    fn draw_big_text(gpu: &mut Gpu, y: i32, s: &str, color: u8) {
        let w = s.chars().count() as i32 * 16;
        let mut x = (SCREEN_W as i32 - w) / 2;
        for ch in s.chars() {
            let g = font::glyph(ch);
            for (row, bits) in g.iter().enumerate() {
                for col in 0..8 {
                    if bits & (0x80 >> col) != 0 {
                        gpu.rect_fill(x + col * 2, y + row as i32 * 2, 2, 2, color);
                    }
                }
            }
            x += 16;
        }
    }

    fn draw_hud(&self, gpu: &mut Gpu) {
        gpu.text(8, 4, "SHIRO", pal::GOFUN);
        gpu.text(SCREEN_W as i32 - 8 - 24, 4, "AKA", pal::BENI);
        for i in 0..IPPON_TO_WIN as i32 {
            let filled0 = self.fighters[0].ippon as i32 > i;
            let filled1 = self.fighters[1].ippon as i32 > i;
            let c0 = if filled0 { pal::GOFUN } else { pal::SUMI };
            let c1 = if filled1 { pal::BENI } else { pal::SUMI };
            gpu.rect_fill(52 + i * 10, 5, 6, 6, c0);
            gpu.rect_fill(SCREEN_W as i32 - 58 - i * 10, 5, 6, 6, c1);
        }
    }
}

impl Game for Ippon {
    fn new(seed: u32) -> Self {
        let mut rng = Rng::new(seed);
        let petals = core::array::from_fn(|_| Ippon::spawn_petal(&mut rng, false));
        Ippon {
            fighters: [
                Fighter::new(START_X[0], 1, false),
                Fighter::new(START_X[1], -1, false),
            ],
            phase: Phase::Title,
            petals,
            rng,
            frame: 0,
            players: 2,
        }
    }

    fn setup(&self, gpu: &mut Gpu) {
        art::load(gpu);
    }

    fn update(&mut self, ctx: &mut UpdateCtx) {
        self.frame += 1;
        self.players = ctx.players.clamp(1, 2);
        self.fighters[1].cpu = self.players == 1;
        self.update_petals();

        let p1 = ctx.input(0);
        let p2 = if self.fighters[1].cpu {
            Input::NONE
        } else {
            ctx.input(1)
        };
        let any_start = p1.start() || p2.start();

        match self.phase {
            Phase::Title => {
                if any_start {
                    ctx.apu.sfx(0, &music::start());
                    for f in &mut self.fighters {
                        f.ippon = 0;
                    }
                    self.reset_round();
                    ctx.apu.play_song(music::dojo_theme());
                    self.phase = Phase::Hajime(HAJIME_FRAMES);
                }
            }
            Phase::Hajime(t) => {
                self.phase = if t == 0 {
                    Phase::Fight
                } else {
                    Phase::Hajime(t - 1)
                };
            }
            Phase::Fight => {
                let i0 = p1;
                let i1 = if self.fighters[1].cpu {
                    self.cpu_input(1)
                } else {
                    p2
                };
                self.fighter_update(0, i0, ctx);
                self.fighter_update(1, i1, ctx);
                self.resolve_strikes(ctx);
            }
            Phase::Point { winner, t } => {
                // Let the strike animation finish during the freeze.
                for f in &mut self.fighters {
                    if matches!(f.stance, Stance::Strike | Stance::Recover) {
                        f.timer += 1;
                        if f.stance == Stance::Strike && f.timer >= STRIKE_FRAMES {
                            f.set(Stance::Recover);
                        }
                    }
                }
                if t == 0 {
                    let [a, b] = self.score();
                    if a >= IPPON_TO_WIN || b >= IPPON_TO_WIN {
                        ctx.apu.stop_song();
                        self.phase = Phase::Victory {
                            winner: u8::from(b > a),
                        };
                    } else {
                        self.reset_round();
                        if !ctx.apu.song_playing() {
                            ctx.apu.play_song(music::dojo_theme());
                        }
                        self.phase = Phase::Hajime(HAJIME_FRAMES);
                    }
                } else {
                    self.phase = Phase::Point { winner, t: t - 1 };
                }
            }
            Phase::Victory { .. } => {
                if any_start {
                    ctx.apu.sfx(0, &music::start());
                    for f in &mut self.fighters {
                        f.ippon = 0;
                    }
                    self.reset_round();
                    self.phase = Phase::Title;
                }
            }
        }
    }

    fn draw(&self, gpu: &mut Gpu) {
        gpu.clear(pal::SUMI);
        // Dojo: wall, shoji, scroll, skirting, floor.
        let bg = &mut gpu.bg[0];
        bg.enabled = true;
        bg.scroll_x = 0;
        bg.scroll_y = 0;
        bg.fill_rect(0, 0, 32, 17, art::tile::WALL);
        bg.fill_rect(0, 17, 32, 1, art::tile::SKIRT);
        for ty in 18..28 {
            for tx in 0..32 {
                let t = if (tx + ty) % 5 == 0 {
                    art::tile::FLOOR_SEAM
                } else {
                    art::tile::FLOOR
                };
                bg.set(tx, ty, t);
            }
        }
        for &tx in &[5, 7, 23, 25] {
            bg.fill_rect(tx, 5, 1, 9, art::tile::SHOJI);
        }
        bg.set(15, 5, art::tile::SCROLL_TOP);
        bg.set(15, 6, art::tile::SCROLL_MID);
        bg.set(15, 7, art::tile::SCROLL_MID);
        bg.set(15, 8, art::tile::SCROLL_BOTTOM);
        bg.set(16, 5, art::tile::SCROLL_TOP);
        bg.set(16, 6, art::tile::SCROLL_MID);
        bg.set(16, 7, art::tile::SCROLL_MID);
        bg.set(16, 8, art::tile::SCROLL_BOTTOM);
        // Pillars in front of the fighters at the edges.
        let fg = &mut gpu.bg[1];
        fg.enabled = true;
        fg.fill(art::tile::EMPTY);
        fg.fill_rect(1, 0, 1, 18, art::tile::PILLAR | PRIORITY);
        fg.fill_rect(30, 0, 1, 18, art::tile::PILLAR | PRIORITY);

        for p in &self.petals {
            gpu.sprite(Sprite::new(p.x.round(), p.y.round(), art::tile::PETAL));
        }
        self.draw_fighter(gpu, 0);
        self.draw_fighter(gpu, 1);
        self.draw_hud(gpu);

        match self.phase {
            Phase::Title => {
                gpu.rect_fill(0, 32, SCREEN_W as i32, 80, pal::SUMI);
                Ippon::draw_big_text(gpu, 38, "IPPON", pal::GOFUN);
                gpu.text_centered(60, "KENDO DUEL", pal::SORA);
                if (self.frame / 30) % 2 == 0 {
                    gpu.text_centered(74, "PRESS START", pal::KI);
                }
                gpu.text_centered(90, "1P ARROWS Z STRIKE X GUARD", pal::NEZUMI);
                gpu.text_centered(100, "2P  WASD  J STRIKE K GUARD", pal::NEZUMI);
                gpu.text_centered(208, "FIRST TO TWO IPPON", pal::KURI);
            }
            Phase::Hajime(t) => {
                if t > HAJIME_FRAMES / 3 {
                    gpu.rect_fill(0, 52, SCREEN_W as i32, 32, pal::SUMI);
                    Ippon::draw_big_text(gpu, 60, "HAJIME!", pal::KI);
                }
            }
            Phase::Fight => {}
            Phase::Point { winner, t } => {
                gpu.rect_fill(0, 44, SCREEN_W as i32, 44, pal::SUMI);
                if winner == 2 {
                    Ippon::draw_big_text(gpu, 58, "AI-UCHI", pal::NEZUMI);
                } else {
                    let (label, color) = if winner == 0 {
                        ("SHIRO", pal::GOFUN)
                    } else {
                        ("AKA", pal::BENI)
                    };
                    Ippon::draw_big_text(gpu, 52, "MEN!", color);
                    gpu.text_centered(76, &format!("IPPON {label}"), color);
                    // The judge's flag rises on the scorer's side.
                    let raise = ((POINT_FRAMES - t).min(20) as i32) / 2;
                    let (fx, tile) = if winner == 0 {
                        (24, art::tile::FLAG_WHITE)
                    } else {
                        (224, art::tile::FLAG_RED)
                    };
                    gpu.sprite(Sprite::new(fx, 120 - raise, tile));
                }
            }
            Phase::Victory { winner } => {
                gpu.rect_fill(0, 44, SCREEN_W as i32, 48, pal::SUMI);
                Ippon::draw_big_text(gpu, 48, "SHOBU ARI", pal::KIN);
                let (label, color) = if winner == 0 {
                    ("SHIRO WINS", pal::GOFUN)
                } else {
                    ("AKA WINS", pal::BENI)
                };
                gpu.text_centered(70, label, color);
                if (self.frame / 30) % 2 == 0 {
                    gpu.text_centered(82, "PRESS START", pal::KI);
                }
            }
        }
    }

    fn hash(&self, h: &mut StateHasher) {
        for f in &self.fighters {
            f.hash(h);
        }
        match self.phase {
            Phase::Title => h.write_u8(0),
            Phase::Hajime(t) => {
                h.write_u8(1);
                h.write_u32(t);
            }
            Phase::Fight => h.write_u8(2),
            Phase::Point { winner, t } => {
                h.write_u8(3);
                h.write_u8(winner);
                h.write_u32(t);
            }
            Phase::Victory { winner } => {
                h.write_u8(4);
                h.write_u8(winner);
            }
        }
        for p in &self.petals {
            h.write_fx(p.x);
            h.write_fx(p.y);
            h.write_fx(p.vy);
            h.write_fx(p.phase);
        }
        self.rng.hash_state(h);
        h.write_u32(self.frame);
        h.write_u8(self.players);
    }

    fn title(&self) -> &'static str {
        "IPPON"
    }

    fn default_players(&self) -> u8 {
        2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagami::{Apu, MAX_PLAYERS};

    fn run(
        game: &mut Ippon,
        apu: &mut Apu,
        frames: u32,
        mut f: impl FnMut(u32) -> [Input; MAX_PLAYERS],
    ) {
        for i in 0..frames {
            let mut ctx = UpdateCtx::new(f(i), i, 2, apu);
            game.update(&mut ctx);
        }
    }

    fn only(p: usize, i: Input) -> [Input; MAX_PLAYERS] {
        let mut a = [Input::NONE; MAX_PLAYERS];
        a[p] = i;
        a
    }

    #[test]
    fn title_to_fight_flow() {
        let mut g = Ippon::new(1);
        let mut apu = Apu::new();
        assert_eq!(g.phase(), Phase::Title);
        run(&mut g, &mut apu, 1, |_| only(1, Input::START));
        assert!(matches!(g.phase(), Phase::Hajime(_)));
        assert!(apu.song_playing());
        run(&mut g, &mut apu, HAJIME_FRAMES + 1, |_| {
            [Input::NONE; MAX_PLAYERS]
        });
        assert_eq!(g.phase(), Phase::Fight);
        assert_eq!(g.title(), "IPPON");
        assert_eq!(g.default_players(), 2);
    }

    #[test]
    fn walking_respects_bounds_and_gap() {
        let mut g = Ippon::new(1);
        let mut apu = Apu::new();
        run(&mut g, &mut apu, 1, |_| only(0, Input::START));
        run(&mut g, &mut apu, HAJIME_FRAMES + 1, |_| {
            [Input::NONE; MAX_PLAYERS]
        });
        // P1 walks right into P2, who stands still: stops MIN_GAP away.
        run(&mut g, &mut apu, 200, |_| only(0, Input::RIGHT));
        let [a, b] = *g.fighters();
        assert_eq!(a.x, b.x - MIN_GAP);
        assert_eq!(a.stance, Stance::Walk);
        assert_eq!(a.facing, 1);
        assert_eq!(b.facing, -1);
        // Both walk to the edges.
        run(&mut g, &mut apu, 400, |_| {
            let mut i = [Input::NONE; MAX_PLAYERS];
            i[0] = Input::LEFT;
            i[1] = Input::RIGHT;
            i
        });
        assert_eq!(g.fighters()[0].x, LEFT_BOUND);
        assert_eq!(g.fighters()[1].x, RIGHT_BOUND);
        run(&mut g, &mut apu, 1, |_| [Input::NONE; MAX_PLAYERS]);
        assert_eq!(g.fighters()[0].stance, Stance::Idle);
    }

    fn fight_ready() -> (Ippon, Apu) {
        let mut g = Ippon::new(1);
        let mut apu = Apu::new();
        run(&mut g, &mut apu, 1, |_| only(0, Input::START));
        run(&mut g, &mut apu, HAJIME_FRAMES + 1, |_| {
            [Input::NONE; MAX_PLAYERS]
        });
        // Close to striking distance.
        run(&mut g, &mut apu, 60, |_| only(0, Input::RIGHT));
        (g, apu)
    }

    #[test]
    fn strike_scores_an_ippon_and_two_win() {
        let (mut g, mut apu) = fight_ready();
        run(&mut g, &mut apu, 1, |_| only(0, Input::A));
        assert_eq!(g.fighters()[0].stance, Stance::Windup);
        run(&mut g, &mut apu, WINDUP_FRAMES + 1, |_| {
            [Input::NONE; MAX_PLAYERS]
        });
        assert!(
            matches!(g.phase(), Phase::Point { winner: 0, .. }),
            "{:?}",
            g.phase()
        );
        assert_eq!(g.score(), [1, 0]);
        assert_eq!(g.fighters()[1].stance, Stance::Hit);
        assert!(!apu.song_playing());
        // Freeze, then a new round.
        run(&mut g, &mut apu, POINT_FRAMES + 1, |_| {
            [Input::NONE; MAX_PLAYERS]
        });
        assert!(matches!(g.phase(), Phase::Hajime(_)));
        assert_eq!(g.fighters()[0].x, Fx::from_int(START_X[0]));
        run(&mut g, &mut apu, HAJIME_FRAMES + 1, |_| {
            [Input::NONE; MAX_PLAYERS]
        });
        run(&mut g, &mut apu, 60, |_| only(0, Input::RIGHT));
        run(&mut g, &mut apu, 1, |_| only(0, Input::A));
        run(&mut g, &mut apu, WINDUP_FRAMES + 1, |_| {
            [Input::NONE; MAX_PLAYERS]
        });
        assert_eq!(g.score(), [2, 0]);
        run(&mut g, &mut apu, POINT_FRAMES + 1, |_| {
            [Input::NONE; MAX_PLAYERS]
        });
        assert_eq!(g.phase(), Phase::Victory { winner: 0 });
        // START returns to the title with scores cleared.
        run(&mut g, &mut apu, 1, |_| only(1, Input::START));
        assert_eq!(g.phase(), Phase::Title);
        assert_eq!(g.score(), [0, 0]);
    }

    #[test]
    fn block_stops_a_strike() {
        let (mut g, mut apu) = fight_ready();
        run(&mut g, &mut apu, 1, |_| {
            let mut i = only(0, Input::A);
            i[1] = Input::B;
            i
        });
        run(&mut g, &mut apu, WINDUP_FRAMES + 2, |_| only(1, Input::B));
        assert_eq!(g.phase(), Phase::Fight);
        assert_eq!(g.score(), [0, 0]);
        assert_eq!(g.fighters()[0].stance, Stance::Recover);
        assert_eq!(g.fighters()[1].stance, Stance::Block);
        // Blocked recovery is longer than a normal one.
        run(&mut g, &mut apu, RECOVER_FRAMES, |_| {
            [Input::NONE; MAX_PLAYERS]
        });
        assert_eq!(g.fighters()[0].stance, Stance::Recover);
        run(&mut g, &mut apu, BLOCKED_RECOVER_FRAMES, |_| {
            [Input::NONE; MAX_PLAYERS]
        });
        assert_eq!(g.fighters()[0].stance, Stance::Idle);
    }

    #[test]
    fn simultaneous_strikes_are_ai_uchi() {
        let (mut g, mut apu) = fight_ready();
        run(&mut g, &mut apu, 1, |_| {
            [Input::A, Input::A, Input::NONE, Input::NONE]
        });
        run(&mut g, &mut apu, WINDUP_FRAMES + 1, |_| {
            [Input::NONE; MAX_PLAYERS]
        });
        assert!(
            matches!(g.phase(), Phase::Point { winner: 2, .. }),
            "{:?}",
            g.phase()
        );
        assert_eq!(g.score(), [0, 0]);
    }

    #[test]
    fn cpu_opponent_plays_in_single_player() {
        let mut g = Ippon::new(7);
        let mut apu = Apu::new();
        let mut ctx = UpdateCtx::new(only(0, Input::START), 0, 1, &mut apu);
        g.update(&mut ctx);
        assert!(g.fighters()[1].cpu);
        let start_x = g.fighters()[1].x;
        for f in 1..600 {
            let mut ctx = UpdateCtx::new([Input::NONE; MAX_PLAYERS], f, 1, &mut apu);
            g.update(&mut ctx);
        }
        // The CPU moved and eventually struck the idle player.
        assert!(
            g.fighters()[1].x != start_x
                || g.score()[1] > 0
                || g.fighters()[0].stance == Stance::Hit
        );
    }

    #[test]
    fn draws_every_phase_without_panicking() {
        let mut gpu = Gpu::new();
        let mut g = Ippon::new(3);
        g.setup(&mut gpu);
        let phases = [
            Phase::Title,
            Phase::Hajime(HAJIME_FRAMES),
            Phase::Fight,
            Phase::Point { winner: 0, t: 50 },
            Phase::Point { winner: 1, t: 5 },
            Phase::Point { winner: 2, t: 5 },
            Phase::Victory { winner: 1 },
        ];
        let mut hashes = Vec::new();
        for p in phases {
            g.phase = p;
            for (i, s) in [Stance::Strike, Stance::Block].into_iter().enumerate() {
                g.fighters[i].stance = s;
            }
            gpu.begin_frame();
            g.draw(&mut gpu);
            gpu.render();
            hashes.push(gpu.frame_hash());
            assert!(gpu.frame().iter().any(|&c| c != pal::SUMI));
        }
        hashes.sort_unstable();
        hashes.dedup();
        assert_eq!(hashes.len(), phases.len(), "every phase looks different");
        // Flipped wide sprite for a left-facing striker.
        g.fighters[1].stance = Stance::Strike;
        gpu.begin_frame();
        g.draw(&mut gpu);
        gpu.render();
        assert_eq!(gpu.sprite_count(), PETALS + 2);
    }

    #[test]
    fn hash_covers_state() {
        let a = Ippon::new(1);
        let mut b = Ippon::new(1);
        let h = |g: &Ippon| {
            let mut s = StateHasher::new();
            g.hash(&mut s);
            s.finish()
        };
        assert_eq!(h(&a), h(&b));
        b.fighters[0].x += Fx::from_raw(1);
        assert_ne!(h(&a), h(&b));
        let c = Ippon::new(2);
        assert_ne!(h(&a), h(&c));
    }
}
