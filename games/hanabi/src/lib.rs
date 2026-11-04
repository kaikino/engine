//! HANABI (花火): a two-player festival-night shooter for KAGAMI.
//!
//! Two yakatabune boats drift down a river under a summer-festival sky,
//! launching firework shells at paper lanterns and oni masks that float
//! down from the night. It is the console's second cartridge and the one
//! that pushes the picture processor: two scrolling layers, a per-scanline
//! reflection on the water, and far more sprites than sixteen per line.
//!
//! Controls: d-pad moves, A fires, B spends a bomb, START begins. One
//! player can play alone; a second boat joins with their START.
#![deny(clippy::float_arithmetic)]

pub mod art;
pub mod music;

#[cfg(target_arch = "wasm32")]
kagami_web::cartridge!(Hanabi);

use kagami::fx::Rect;
use kagami::gpu::{Sprite, SCREEN_H, SCREEN_W};
use kagami::input::Edge;
use kagami::{palette as pal, Fx, Game, Gpu, Input, Rng, StateHasher, UpdateCtx, Vec2};

const RIVER_TOP: i32 = 152;
const PLAY_LEFT: Fx = Fx::from_int(8);
const PLAY_RIGHT: Fx = Fx::from_int(SCREEN_W as i32 - 24);
const BOAT_SPEED: Fx = Fx::from_ratio(7, 4);
const SHOT_SPEED: Fx = Fx::from_int(-4);
const MAX_SHOTS: usize = 24;
const MAX_ENEMIES: usize = 40;
const MAX_ENEMY_SHOTS: usize = 24;
const MAX_SPARKS: usize = 48;
const START_LIVES: u8 = 3;
const START_BOMBS: u8 = 2;
const RESPAWN_FRAMES: u32 = 90;
const INVULN_FRAMES: u32 = 120;
const WAVE_FRAMES: u32 = 180;
const SRAM_MAGIC: &[u8; 4] = b"HNBI";

/// Match flow.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Title screen.
    Title,
    /// Playing.
    Play,
    /// Everyone is out of boats; shows the score.
    GameOver {
        /// Frames remaining before START is accepted.
        t: u32,
    },
}

/// Enemy kinds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Drifts down, sways, worth 100.
    Lantern,
    /// Weaves across, shoots, takes 3 hits, worth 500.
    Oni,
}

#[derive(Clone, Copy, Debug)]
struct Boat {
    pos: Vec2,
    alive: bool,
    joined: bool,
    lives: u8,
    bombs: u8,
    score: u32,
    cooldown: u32,
    respawn: u32,
    invuln: u32,
    edge: Edge,
}

impl Boat {
    fn new(x: i32) -> Boat {
        Boat {
            pos: Vec2::from_ints(x, RIVER_TOP + 24),
            alive: false,
            joined: false,
            lives: START_LIVES,
            bombs: START_BOMBS,
            score: 0,
            cooldown: 0,
            respawn: 0,
            invuln: 0,
            edge: Edge::default(),
        }
    }

    fn hurt_box(&self) -> Rect {
        Rect::new(self.pos + Vec2::from_ints(4, 4), Vec2::from_ints(8, 8))
    }

    fn hash(&self, h: &mut StateHasher) {
        h.write_vec2(self.pos);
        h.write_bool(self.alive);
        h.write_bool(self.joined);
        h.write_u8(self.lives);
        h.write_u8(self.bombs);
        h.write_u32(self.score);
        h.write_u32(self.cooldown);
        h.write_u32(self.respawn);
        h.write_u32(self.invuln);
        h.write_u8(self.edge.current().bits());
    }
}

#[derive(Clone, Copy, Debug)]
struct Shot {
    pos: Vec2,
    owner: u8,
}

#[derive(Clone, Copy, Debug)]
struct Enemy {
    kind: Kind,
    pos: Vec2,
    phase: Fx,
    hp: u8,
    age: u32,
}

impl Enemy {
    fn hit_box(&self) -> Rect {
        match self.kind {
            Kind::Lantern => Rect::new(self.pos + Vec2::from_ints(1, 1), Vec2::from_ints(6, 6)),
            Kind::Oni => Rect::new(self.pos + Vec2::from_ints(2, 2), Vec2::from_ints(12, 12)),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Spark {
    pos: Vec2,
    vel: Vec2,
    life: u8,
    big: bool,
}

/// The game.
#[derive(Clone, Debug)]
pub struct Hanabi {
    boats: [Boat; 2],
    shots: Vec<Shot>,
    enemies: Vec<Enemy>,
    enemy_shots: Vec<Shot>,
    sparks: Vec<Spark>,
    phase: Phase,
    rng: Rng,
    frame: u32,
    wave: u32,
    wave_timer: u32,
    scroll: Fx,
    flash: u32,
    bloom: Option<(Vec2, u32)>,
    players: u8,
    /// Best score from save data: a record, never hashed.
    high_score: u32,
}

impl Hanabi {
    /// Current phase.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Scores for both boats.
    pub fn scores(&self) -> [u32; 2] {
        [self.boats[0].score, self.boats[1].score]
    }

    /// Lives for both boats.
    pub fn lives(&self) -> [u8; 2] {
        [self.boats[0].lives, self.boats[1].lives]
    }

    /// Enemies on screen.
    pub fn enemy_count(&self) -> usize {
        self.enemies.len()
    }

    /// Current wave number.
    pub fn wave(&self) -> u32 {
        self.wave
    }

    /// Best score from save data.
    pub fn high_score(&self) -> u32 {
        self.high_score
    }

    fn start_match(&mut self) {
        for (i, b) in self.boats.iter_mut().enumerate() {
            *b = Boat::new(if i == 0 { 96 } else { 144 });
        }
        self.shots.clear();
        self.enemies.clear();
        self.enemy_shots.clear();
        self.sparks.clear();
        self.wave = 0;
        self.wave_timer = 60;
        self.bloom = None;
        self.phase = Phase::Play;
    }

    fn join(&mut self, i: usize) {
        let b = &mut self.boats[i];
        if !b.joined {
            b.joined = true;
            b.alive = true;
            b.invuln = INVULN_FRAMES;
        }
    }

    fn spawn_wave(&mut self) {
        self.wave += 1;
        let n = (6 + self.wave * 2).min(18) as usize;
        let formation = self.wave % 3;
        for i in 0..n {
            if self.enemies.len() >= MAX_ENEMIES {
                break;
            }
            let (x, y) = match formation {
                0 => (16 + (i as i32 % 9) * 26, -12 - (i as i32 / 9) * 14),
                1 => (24 + i as i32 * 12, -12 - (i as i32 % 2) * 10),
                _ => (
                    128 + ((i as i32 % 2) * 2 - 1) * (i as i32 / 2 + 1) * 14,
                    -10 - i as i32 * 4,
                ),
            };
            self.enemies.push(Enemy {
                kind: Kind::Lantern,
                pos: Vec2::from_ints(x, y),
                phase: self.rng.fx(),
                hp: 1,
                age: 0,
            });
        }
        if self.wave >= 2 && self.enemies.len() < MAX_ENEMIES {
            let count = if self.wave >= 5 { 2 } else { 1 };
            for j in 0..count {
                self.enemies.push(Enemy {
                    kind: Kind::Oni,
                    pos: Vec2::from_ints(64 + j * 112, -24),
                    phase: self.rng.fx(),
                    hp: 3,
                    age: 0,
                });
            }
        }
    }

    fn burst_at(&mut self, pos: Vec2, big: bool) {
        let n = if big { 12 } else { 6 };
        for k in 0..n {
            if self.sparks.len() >= MAX_SPARKS {
                break;
            }
            let dir = Vec2::from_turns(Fx::from_ratio(k, n) + self.rng.fx() / 8);
            let speed = if big {
                Fx::from_ratio(5, 2)
            } else {
                Fx::from_ratio(3, 2)
            };
            self.sparks.push(Spark {
                pos,
                vel: dir * speed,
                life: if big { 28 } else { 18 },
                big,
            });
        }
    }

    fn update_boat(&mut self, i: usize, input: Input, ctx: &mut UpdateCtx) {
        let b = &mut self.boats[i];
        b.edge.update(input);
        if !b.joined {
            if b.edge.pressed(Input::START) && b.lives > 0 {
                self.join(i);
            }
            return;
        }
        if !b.alive {
            if b.respawn > 0 {
                b.respawn -= 1;
                if b.respawn == 0 && b.lives > 0 {
                    b.alive = true;
                    b.invuln = INVULN_FRAMES;
                    b.pos = Vec2::from_ints(if i == 0 { 96 } else { 144 }, RIVER_TOP + 24);
                }
            }
            return;
        }
        b.invuln = b.invuln.saturating_sub(1);
        b.cooldown = b.cooldown.saturating_sub(1);
        let dx = Fx::from_int(input.axis_x()) * BOAT_SPEED;
        let dy = Fx::from_int(input.axis_y()) * BOAT_SPEED;
        b.pos.x = (b.pos.x + dx).clamp(PLAY_LEFT, PLAY_RIGHT);
        b.pos.y = (b.pos.y + dy).clamp(
            Fx::from_int(RIVER_TOP - 8),
            Fx::from_int(SCREEN_H as i32 - 24),
        );
        if input.a() && b.cooldown == 0 && self.shots.len() < MAX_SHOTS {
            b.cooldown = 8;
            let pos = b.pos + Vec2::from_ints(4, -4);
            self.shots.push(Shot {
                pos,
                owner: i as u8,
            });
            ctx.apu.sfx(1, &music::shoot());
        }
        if b.edge.pressed(Input::B) && b.bombs > 0 && self.bloom.is_none() {
            b.bombs -= 1;
            let center = Vec2::from_ints(b.pos.x.round() - 8, 48);
            self.bloom = Some((center, 36));
            ctx.apu.sfx(0, &music::bomb());
            self.flash = 6;
            self.wave_timer = self.wave_timer.max(60);
            // Everything on screen pops.
            let dead: Vec<Enemy> = std::mem::take(&mut self.enemies);
            self.enemy_shots.clear();
            for e in dead {
                self.boats[i].score += if e.kind == Kind::Oni { 500 } else { 100 };
                self.burst_at(e.pos + Vec2::from_ints(4, 4), false);
            }
        }
    }

    fn update_world(&mut self, ctx: &mut UpdateCtx) {
        // Scroll the river.
        self.scroll += Fx::from_ratio(1, 2);
        if self.scroll >= Fx::from_int(256) {
            self.scroll -= Fx::from_int(256);
        }
        self.flash = self.flash.saturating_sub(1);
        if let Some((_, t)) = &mut self.bloom {
            *t -= 1;
            if *t == 0 {
                self.bloom = None;
            }
        }
        // Waves: on a timer, brought forward when the screen is clear.
        if self.enemies.is_empty() {
            self.wave_timer = self.wave_timer.min(45);
        }
        if self.wave_timer == 0 {
            self.spawn_wave();
            self.wave_timer = WAVE_FRAMES;
        } else {
            self.wave_timer -= 1;
        }
        // Player shots.
        for s in &mut self.shots {
            s.pos.y += SHOT_SPEED;
        }
        self.shots.retain(|s| s.pos.y > Fx::from_int(-8));
        // Enemies.
        let mut new_shots: Vec<Shot> = Vec::new();
        let boats_alive: Vec<Vec2> = self
            .boats
            .iter()
            .filter(|b| b.alive)
            .map(|b| b.pos)
            .collect();
        let wave = self.wave;
        for e in &mut self.enemies {
            e.age += 1;
            e.phase += Fx::from_ratio(1, 120);
            match e.kind {
                Kind::Lantern => {
                    e.pos.y += Fx::from_ratio(1, 2) + Fx::from_int(wave as i32) / 24;
                    e.pos.x += Fx::sin_turns(e.phase) * Fx::from_ratio(3, 4);
                }
                Kind::Oni => {
                    if e.pos.y < Fx::from_int(40) {
                        e.pos.y += Fx::from_ratio(3, 4);
                    }
                    e.pos.x += Fx::sin_turns(e.phase * 2) * Fx::from_ratio(5, 4);
                    if e.age % 50 == 0 && new_shots.len() + self.enemy_shots.len() < MAX_ENEMY_SHOTS
                    {
                        if let Some(target) = boats_alive.first() {
                            let _ = target;
                            new_shots.push(Shot {
                                pos: e.pos + Vec2::from_ints(4, 12),
                                owner: 2,
                            });
                        }
                    }
                }
            }
        }
        if !new_shots.is_empty() {
            ctx.apu.sfx(2, &music::shoot());
        }
        self.enemy_shots.extend(new_shots);
        for s in &mut self.enemy_shots {
            s.pos.y += Fx::from_ratio(3, 2);
        }
        self.enemy_shots
            .retain(|s| s.pos.y < Fx::from_int(SCREEN_H as i32));
        // Enemies that drift past the river leave.
        self.enemies
            .retain(|e| e.pos.y < Fx::from_int(SCREEN_H as i32));
        // Shots hit enemies.
        let mut popped: Vec<(Vec2, bool)> = Vec::new();
        let mut scored: Vec<(u8, u32)> = Vec::new();
        let mut i = 0;
        while i < self.shots.len() {
            let shot = self.shots[i];
            let shot_box = Rect::new(shot.pos + Vec2::from_ints(2, 0), Vec2::from_ints(4, 6));
            let mut hit = None;
            for (j, e) in self.enemies.iter_mut().enumerate() {
                if e.hit_box().overlaps(&shot_box) {
                    e.hp = e.hp.saturating_sub(1);
                    hit = Some(j);
                    break;
                }
            }
            match hit {
                Some(j) => {
                    self.shots.swap_remove(i);
                    let e = self.enemies[j];
                    if e.hp == 0 {
                        self.enemies.swap_remove(j);
                        let big = e.kind == Kind::Oni;
                        popped.push((e.pos + Vec2::from_ints(if big { 8 } else { 4 }, 4), big));
                        scored.push((shot.owner, if big { 500 } else { 100 }));
                    } else {
                        self.flash = self.flash.max(1);
                    }
                }
                None => i += 1,
            }
        }
        for (pos, big) in popped {
            self.burst_at(pos, big);
            ctx.apu
                .sfx(3, &if big { music::burst() } else { music::pop() });
        }
        for (owner, pts) in scored {
            if let Some(b) = self.boats.get_mut(owner as usize) {
                b.score += pts;
            }
        }
        // Enemies and their shots hit boats.
        for i in 0..2 {
            let b = self.boats[i];
            if !b.alive || b.invuln > 0 {
                continue;
            }
            let hb = b.hurt_box();
            let struck = self.enemies.iter().any(|e| e.hit_box().overlaps(&hb))
                || self.enemy_shots.iter().any(|s| {
                    Rect::new(s.pos + Vec2::from_ints(2, 2), Vec2::from_ints(4, 4)).overlaps(&hb)
                });
            if struck {
                let boat = &mut self.boats[i];
                boat.alive = false;
                boat.lives -= 1;
                boat.respawn = RESPAWN_FRAMES;
                let pos = boat.pos + Vec2::from_ints(8, 8);
                self.burst_at(pos, true);
                ctx.apu.sfx(0, &music::sink());
                self.flash = 4;
            }
        }
        // Sparks.
        for s in &mut self.sparks {
            s.pos += s.vel;
            s.vel.y += Fx::from_ratio(1, 16);
            s.life -= 1;
        }
        self.sparks.retain(|s| s.life > 0);
        // Game over when nobody can come back.
        let anyone_left = self
            .boats
            .iter()
            .any(|b| b.joined && (b.alive || b.lives > 0));
        if !anyone_left {
            ctx.apu.stop_song();
            self.phase = Phase::GameOver { t: 120 };
        }
    }

    fn draw_background(&self, gpu: &mut Gpu) {
        let scroll = self.scroll.round();
        let sky = &mut gpu.bg[0];
        sky.enabled = true;
        sky.scroll_x = 0;
        sky.scroll_y = -scroll;
        sky.window = None;
        sky.clear_line_scroll();
        for ty in 0..32 {
            for tx in 0..32 {
                let t = match (tx * 7 + ty * 13) % 11 {
                    0 => art::tile::SKY_STAR,
                    4 | 8 => art::tile::SKY_DIM,
                    _ => art::tile::SKY,
                };
                sky.set(tx, ty, t);
            }
        }
        // The river layer: a mountain ridge above the bank, the lantern
        // string, and water with a wavy per-line reflection.
        let river = &mut gpu.bg[1];
        river.enabled = true;
        river.scroll_x = 0;
        river.scroll_y = 0;
        river.fill(art::tile::EMPTY);
        let ridge = RIVER_TOP / 8 - 2;
        for tx in 0..32 {
            let h = 1 + ((tx * 5) % 7 == 0) as i32;
            for k in 0..h {
                let ty = ridge - k;
                let t = if k == h - 1 {
                    if tx % 2 == 0 {
                        art::tile::MOUNTAIN_L
                    } else {
                        art::tile::MOUNTAIN_R
                    }
                } else {
                    art::tile::MOUNTAIN
                };
                river.set(tx, ty, t);
            }
        }
        river.fill_rect(0, RIVER_TOP / 8 - 1, 32, 1, art::tile::LANTERN_STRING);
        for ty in RIVER_TOP / 8..28 {
            for tx in 0..32 {
                let t = if (tx * 3 + ty * 5 + (self.frame / 20) as i32) % 7 == 0 {
                    art::tile::RIVER_GLINT
                } else {
                    art::tile::RIVER
                };
                river.set(tx, ty, t);
            }
        }
        river.fill_rect(0, 27, 32, 1, art::tile::BANK);
        let t = Fx::from_int(self.frame as i32) / 60;
        river.set_line_scroll(move |y| {
            if y as i32 >= RIVER_TOP {
                (Fx::sin_turns(t + Fx::from_int(y as i32) / 12) * Fx::from_int(2)).round()
            } else {
                0
            }
        });
    }

    fn draw_hud(&self, gpu: &mut Gpu) {
        for (i, b) in self.boats.iter().enumerate() {
            let x = if i == 0 { 4 } else { SCREEN_W as i32 - 4 - 96 };
            let color = if i == 0 { pal::KIN } else { pal::MOMO };
            if b.joined {
                gpu.text(x, 2, &format!("{}P {:06}", i + 1, b.score), color);
                for l in 0..b.lives.min(5) as i32 {
                    gpu.blit_tile(x + l * 9, 11, art::tile::LIFE, false, false);
                }
                for k in 0..b.bombs.min(3) as i32 {
                    gpu.blit_tile(x + 56 + k * 9, 11, art::tile::BOMB, false, false);
                }
            } else if self.phase == Phase::Play && b.lives > 0 && (self.frame / 30) % 2 == 0 {
                let key = if i == 0 { "ENTER" } else { "TAB" };
                gpu.text_centered(212, &format!("{}P JOINS WITH {key}", i + 1), pal::SUNA);
            }
        }
        gpu.text_centered(
            2,
            &format!(
                "HI {:06}",
                self.high_score.max(self.scores()[0]).max(self.scores()[1])
            ),
            pal::GOFUN,
        );
    }
}

impl Game for Hanabi {
    fn new(seed: u32) -> Self {
        Hanabi {
            boats: [Boat::new(96), Boat::new(144)],
            shots: Vec::new(),
            enemies: Vec::new(),
            enemy_shots: Vec::new(),
            sparks: Vec::new(),
            phase: Phase::Title,
            rng: Rng::new(seed),
            frame: 0,
            wave: 0,
            wave_timer: 0,
            scroll: Fx::ZERO,
            flash: 0,
            bloom: None,
            players: 2,
            high_score: 0,
        }
    }

    fn setup(&self, gpu: &mut Gpu) {
        art::load(gpu);
    }

    fn load_sram(&mut self, data: &[u8]) {
        if data.len() >= 8 && &data[..4] == SRAM_MAGIC {
            self.high_score = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        }
    }

    fn save_sram(&self) -> Option<Vec<u8>> {
        let best = self.high_score.max(self.scores()[0]).max(self.scores()[1]);
        let mut v = SRAM_MAGIC.to_vec();
        v.extend_from_slice(&best.to_le_bytes());
        Some(v)
    }

    fn update(&mut self, ctx: &mut UpdateCtx) {
        self.frame += 1;
        self.players = ctx.players.clamp(1, 2);
        match self.phase {
            Phase::Title => {
                self.scroll += Fx::from_ratio(1, 4);
                if self.scroll >= Fx::from_int(256) {
                    self.scroll -= Fx::from_int(256);
                }
                let starters: Vec<usize> = (0..self.players as usize)
                    .filter(|&i| ctx.input(i).start())
                    .collect();
                if let Some(&first) = starters.first() {
                    self.start_match();
                    self.join(first);
                    ctx.apu.sfx(0, &music::start());
                    ctx.apu.play_song(music::festival());
                }
            }
            Phase::Play => {
                for i in 0..self.players as usize {
                    let input = ctx.input(i);
                    self.update_boat(i, input, ctx);
                }
                self.update_world(ctx);
            }
            Phase::GameOver { t } => {
                for s in &mut self.sparks {
                    s.pos += s.vel;
                    s.life = s.life.saturating_sub(1);
                }
                self.sparks.retain(|s| s.life > 0);
                if t > 0 {
                    self.phase = Phase::GameOver { t: t - 1 };
                } else if (0..self.players as usize).any(|i| ctx.input(i).start()) {
                    self.high_score = self.high_score.max(self.scores()[0]).max(self.scores()[1]);
                    self.phase = Phase::Title;
                    for b in &mut self.boats {
                        b.edge.update(Input::NONE);
                    }
                }
            }
        }
    }

    fn draw(&self, gpu: &mut Gpu) {
        gpu.clear(pal::KON);
        gpu.set_color_key(pal::SUMI);
        self.draw_background(gpu);
        gpu.pal_reset();
        if self.flash > 0 {
            gpu.pal(pal::KON, pal::AI);
            gpu.pal(pal::AI, pal::ASAGI);
        }
        gpu.set_mosaic(match self.phase {
            Phase::GameOver { t } => (1 + (120u32.saturating_sub(t)) / 20).min(6) as u8,
            _ => 1,
        });
        // Sprites: boats first (top priority), then shots, enemies, sparks.
        for (i, b) in self.boats.iter().enumerate() {
            if b.alive && (b.invuln == 0 || (self.frame / 4) % 2 == 0) {
                let tile = if i == 0 {
                    art::sprite::BOAT
                } else {
                    art::sprite::BOAT2
                };
                gpu.sprite(Sprite::new(b.pos.x.round(), b.pos.y.round(), tile).size(2, 2));
            }
        }
        for s in &self.shots {
            gpu.sprite(Sprite::new(
                s.pos.x.round(),
                s.pos.y.round(),
                art::tile::SHOT,
            ));
        }
        for s in &self.enemy_shots {
            gpu.sprite(Sprite::new(
                s.pos.x.round(),
                s.pos.y.round(),
                art::tile::ENEMY_SHOT,
            ));
        }
        for e in &self.enemies {
            let (tile, w) = match e.kind {
                Kind::Lantern => (
                    if (e.age / 15) % 2 == 0 {
                        art::sprite::LANTERN_A
                    } else {
                        art::sprite::LANTERN_B
                    },
                    1,
                ),
                Kind::Oni => (art::sprite::ONI, 2),
            };
            gpu.sprite(Sprite::new(e.pos.x.round(), e.pos.y.round(), tile).size(w, w));
        }
        for s in &self.sparks {
            if s.big && s.life > 12 {
                let tile = if (s.life / 4) % 2 == 0 {
                    art::sprite::BURST_A
                } else {
                    art::sprite::BURST_B
                };
                gpu.sprite(Sprite::new(s.pos.x.round() - 8, s.pos.y.round() - 8, tile).size(2, 2));
            } else {
                gpu.sprite(Sprite::new(
                    s.pos.x.round() - 4,
                    s.pos.y.round() - 4,
                    art::tile::SPARK,
                ));
            }
        }
        if let Some((pos, t)) = self.bloom {
            gpu.sprite(
                Sprite::new(pos.x.round() - 8, pos.y.round() - 8, art::sprite::BLOOM)
                    .size(4, 4)
                    .flip_x((t / 3) % 2 == 1),
            );
        }
        self.draw_hud(gpu);
        match self.phase {
            Phase::Title => {
                gpu.rect_fill(0, 56, SCREEN_W as i32, 64, pal::SUMI);
                gpu.text_centered(64, "ハナビ", pal::KIN);
                gpu.text_centered(76, "H A N A B I", pal::GOFUN);
                gpu.text_centered(92, "FESTIVAL NIGHT SHOOTER", pal::SORA);
                if (self.frame / 30) % 2 == 0 {
                    gpu.text_centered(106, "PRESS START", pal::KI);
                }
                gpu.text_centered(196, "ARROWS MOVE  Z FIRE  X BOMB", pal::SUNA);
                gpu.text_centered(206, "2P JOINS WITH TAB", pal::SUNA);
            }
            Phase::Play => {
                if self.wave_timer > WAVE_FRAMES - 40 && self.wave > 0 {
                    gpu.text_centered(70, &format!("WAVE {}", self.wave), pal::KI);
                }
            }
            Phase::GameOver { .. } => {
                gpu.rect_fill(0, 72, SCREEN_W as i32, 40, pal::SUMI);
                gpu.text_centered(80, "ゲームオーバー", pal::BENI);
                let [a, b] = self.scores();
                gpu.text_centered(96, &format!("1P {a:06}   2P {b:06}"), pal::GOFUN);
            }
        }
    }

    fn hash(&self, h: &mut StateHasher) {
        for b in &self.boats {
            b.hash(h);
        }
        h.write_usize(self.shots.len());
        for s in &self.shots {
            h.write_vec2(s.pos);
            h.write_u8(s.owner);
        }
        h.write_usize(self.enemies.len());
        for e in &self.enemies {
            h.write_u8(u8::from(e.kind == Kind::Oni));
            h.write_vec2(e.pos);
            h.write_fx(e.phase);
            h.write_u8(e.hp);
            h.write_u32(e.age);
        }
        h.write_usize(self.enemy_shots.len());
        for s in &self.enemy_shots {
            h.write_vec2(s.pos);
        }
        h.write_usize(self.sparks.len());
        for s in &self.sparks {
            h.write_vec2(s.pos);
            h.write_vec2(s.vel);
            h.write_u8(s.life);
            h.write_bool(s.big);
        }
        match self.phase {
            Phase::Title => h.write_u8(0),
            Phase::Play => h.write_u8(1),
            Phase::GameOver { t } => {
                h.write_u8(2);
                h.write_u32(t);
            }
        }
        self.rng.hash_state(h);
        h.write_u32(self.frame);
        h.write_u32(self.wave);
        h.write_u32(self.wave_timer);
        h.write_fx(self.scroll);
        h.write_u32(self.flash);
        if let Some((p, t)) = self.bloom {
            h.write_vec2(p);
            h.write_u32(t);
        }
        h.write_u8(self.players);
    }

    fn title(&self) -> &'static str {
        "HANABI"
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
        g: &mut Hanabi,
        apu: &mut Apu,
        frames: u32,
        mut f: impl FnMut(u32) -> [Input; MAX_PLAYERS],
    ) {
        for i in 0..frames {
            let mut ctx = UpdateCtx::new(f(i), i, 2, apu);
            g.update(&mut ctx);
        }
    }

    fn p1(i: Input) -> [Input; MAX_PLAYERS] {
        let mut a = [Input::NONE; MAX_PLAYERS];
        a[0] = i;
        a
    }

    #[test]
    fn title_start_play_and_second_player_joins() {
        let mut g = Hanabi::new(1);
        let mut apu = Apu::new();
        assert_eq!(g.phase(), Phase::Title);
        run(&mut g, &mut apu, 1, |_| p1(Input::START));
        assert_eq!(g.phase(), Phase::Play);
        assert!(apu.song_playing());
        assert!(g.boats[0].joined && !g.boats[1].joined);
        run(&mut g, &mut apu, 2, |_| {
            [Input::NONE, Input::START, Input::NONE, Input::NONE]
        });
        assert!(g.boats[1].joined);
        assert_eq!(g.title(), "HANABI");
        assert_eq!(g.default_players(), 2);
    }

    #[test]
    fn shooting_scores_and_waves_advance() {
        let mut g = Hanabi::new(7);
        let mut apu = Apu::new();
        run(&mut g, &mut apu, 1, |_| p1(Input::START));
        // Hold fire and sweep: within a few seconds something must pop.
        run(&mut g, &mut apu, 900, |f| {
            p1(Input::A
                | if (f / 40) % 2 == 0 {
                    Input::LEFT
                } else {
                    Input::RIGHT
                })
        });
        assert!(g.scores()[0] > 0, "no lanterns hit: {:?}", g.scores());
        assert!(g.wave() >= 2);
        assert!(g.enemy_count() <= MAX_ENEMIES);
    }

    #[test]
    fn bomb_clears_the_screen_once_per_bomb() {
        let mut g = Hanabi::new(3);
        let mut apu = Apu::new();
        run(&mut g, &mut apu, 1, |_| p1(Input::START));
        run(&mut g, &mut apu, 200, |_| p1(Input::NONE));
        assert!(g.enemy_count() > 0);
        let before = g.scores()[0];
        run(&mut g, &mut apu, 1, |_| p1(Input::B));
        assert_eq!(g.enemy_count(), 0);
        assert!(g.scores()[0] > before);
        assert_eq!(g.boats[0].bombs, START_BOMBS - 1);
        assert!(g.bloom.is_some());
        run(&mut g, &mut apu, 1, |_| p1(Input::NONE));
        run(&mut g, &mut apu, 1, |_| p1(Input::B));
        assert_eq!(
            g.boats[0].bombs,
            START_BOMBS - 1,
            "no second bomb while one blooms"
        );
    }

    #[test]
    fn idle_player_eventually_loses_all_boats() {
        let mut g = Hanabi::new(5);
        let mut apu = Apu::new();
        run(&mut g, &mut apu, 1, |_| p1(Input::START));
        for f in 1..20_000 {
            let mut ctx = UpdateCtx::new([Input::NONE; MAX_PLAYERS], f, 2, &mut apu);
            g.update(&mut ctx);
            if matches!(g.phase(), Phase::GameOver { .. }) {
                assert_eq!(g.lives()[0], 0);
                // After the freeze, START returns to the title and keeps the record.
                run(&mut g, &mut apu, 121, |_| p1(Input::NONE));
                run(&mut g, &mut apu, 1, |_| p1(Input::START));
                assert_eq!(g.phase(), Phase::Title);
                assert_eq!(g.high_score(), g.scores()[0]);
                return;
            }
        }
        panic!("never lost");
    }

    #[test]
    fn sram_roundtrip_and_record_is_not_hashed() {
        let mut g = Hanabi::new(1);
        g.load_sram(b"nope");
        assert_eq!(g.high_score(), 0);
        g.load_sram(&[b'H', b'N', b'B', b'I', 0x10, 0x27, 0, 0]);
        assert_eq!(g.high_score(), 10_000);
        let saved = g.save_sram().unwrap();
        let mut h = Hanabi::new(1);
        h.load_sram(&saved);
        assert_eq!(h.high_score(), 10_000);
        let hash = |x: &Hanabi| {
            let mut s = StateHasher::new();
            x.hash(&mut s);
            s.finish()
        };
        assert_eq!(hash(&g), hash(&Hanabi::new(1)));
    }

    #[test]
    fn draws_every_phase_and_exceeds_the_scanline_budget() {
        let mut gpu = Gpu::new();
        let mut g = Hanabi::new(9);
        g.setup(&mut gpu);
        let mut apu = Apu::new();
        gpu.begin_frame();
        g.draw(&mut gpu);
        gpu.render();
        let title_hash = gpu.frame_hash();
        run(&mut g, &mut apu, 1, |_| p1(Input::START));
        // Run until a wave is dense enough that the per-line limit bites.
        let mut dropped = 0;
        for _ in 0..1200 {
            run(&mut g, &mut apu, 1, |_| p1(Input::A));
            gpu.begin_frame();
            g.draw(&mut gpu);
            gpu.render();
            dropped = dropped.max(gpu.dropped_sprite_lines());
        }
        assert!(
            dropped > 0,
            "HANABI is supposed to overrun 16 sprites per line"
        );
        assert_ne!(gpu.frame_hash(), title_hash);
        g.phase = Phase::GameOver { t: 30 };
        gpu.begin_frame();
        g.draw(&mut gpu);
        gpu.render();
        assert!(gpu.mosaic() > 1);
    }

    #[test]
    fn hash_covers_state() {
        let a = Hanabi::new(1);
        let mut b = Hanabi::new(1);
        let hash = |x: &Hanabi| {
            let mut s = StateHasher::new();
            x.hash(&mut s);
            s.finish()
        };
        assert_eq!(hash(&a), hash(&b));
        b.scroll += Fx::from_raw(1);
        assert_ne!(hash(&a), hash(&b));
        assert_ne!(hash(&a), hash(&Hanabi::new(2)));
    }
}
