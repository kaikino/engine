# Cookbook

Patterns that come up in every KAGAMI cartridge, with the determinism
angle spelled out. IPPON and HANABI use all of them; grep their sources
for working code.

## State machines with timers

Keep phase and timer together in one enum so a frame can never be in two
phases at once, and hash the whole thing:

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase { Title, Countdown(u32), Play, Over { winner: u8, t: u32 } }

fn hash(&self, h: &mut StateHasher) {
    match self.phase {
        Phase::Title => h.write_u8(0),
        Phase::Countdown(t) => { h.write_u8(1); h.write_u32(t); }
        Phase::Play => h.write_u8(2),
        Phase::Over { winner, t } => { h.write_u8(3); h.write_u8(winner); h.write_u32(t); }
    }
}
```

Write the tag byte first; two variants with equal payloads must not hash
alike.

## Edge-triggered buttons

`Input` is level state. For "pressed this frame", keep an `Edge` per
player in your game state and feed it every frame, including frames where
you ignore the input, or presses leak into the next phase:

```rust
self.edge[p].update(ctx.input(p));
if self.edge[p].pressed(Input::A) { self.jump(); }
```

`Edge` is two bytes; hash `edge.current().bits()`.

## Collections: order is everything

`Vec` iterated front to back is deterministic. `swap_remove` is fine as
long as every peer does the same `swap_remove` on the same index, which
they will if the decision came from state. Never iterate a `HashMap` or
`HashSet` in `update`; their order changes between processes. If you need
a map, use `BTreeMap`.

Hash a `Vec` by writing its length and then each element. Two lists with
the same elements in different orders are different states.

## Random numbers

One `Rng` in the game, seeded from the match seed, hashed with
`rng.hash_state(h)`. Draw from it in a fixed order. A tempting mistake is
to draw "only when needed" inside `draw`; `draw` takes `&self` so the
compiler stops you, which is the point.

For cosmetic randomness that must not affect gameplay (a petal's sway),
drawing from the same `Rng` is still correct: it is state, it is hashed,
every peer draws it identically. What you must not do is let it depend on
anything a peer might lack, such as which frames were rendered.

## Fixed-point motion

```rust
const SPEED: Fx = Fx::from_ratio(7, 4);         // 1.75 px/frame
self.pos.x += Fx::from_int(input.axis_x()) * SPEED;
self.pos.x = self.pos.x.clamp(LEFT, RIGHT);
let screen_x = self.pos.x.round();              // only at draw time
```

Keep velocities and positions in `Fx`; convert to `i32` only for the
sprite's screen position. `Fx::sin_turns` takes turns, not radians, so a
full swing is `phase += Fx::from_ratio(1, 90)` for a 90-frame cycle.

## Hitboxes

`Rect::overlaps` treats touching edges as not overlapping. Pick hitboxes
smaller than the sprite; HANABI's boat is 8×8 inside a 16×16 sprite. For
one-directional attacks (IPPON's strike) express the reach as an x range in
the attacker's facing and compare against the defender's box.

## Sound from `update`, never from `draw`

Trigger effects where the event happens, in `update`:

```rust
if landed { ctx.apu.sfx(0, &music::hit()); }
```

That puts sound on the deterministic timeline: replays sound right and
rollback uses a scratch processor during re-simulation so effects do not
double-fire. Starting a song on a phase transition is the same rule.

## Sprite budget

Sixteen sprites per scanline. A row of enemies at the same height is the
classic way to hit it. Options, in order of preference:

1. Stagger heights by a few pixels so no single line carries them all.
2. Check `gpu.dropped_sprite_lines()` after `render` during development.
3. Flicker on purpose: alternate which half of a crowd you push each frame.
4. Use the overlay (`blit_tile`) for HUD icons so they cost no budget.

## Save data is not state

Anything from `load_sram` stays out of `update` and `hash`. Show it,
update it on match end, save it in `save_sram`. See decision D15.

## Debugging a desync

1. The desync report names the first checkpoint that disagreed; the bug is
   in the preceding `hash_interval` frames.
2. Save a replay on one peer (F4) and `--verify` it on the other. The first
   failing checkpoint narrows it to 60 frames.
3. In a test, re-simulate both builds frame by frame with
   `kagami::game::simulate` and compare hashes to find the exact frame.
4. The usual suspects: a field missing from `hash`, a float, a `HashMap`,
   a clock, or reading `ctx.players` differently on each side.

## Testing a cartridge

- A unit test per rule ("a block stops a strike"), driving `update` with
  hand-made inputs.
- A determinism test: simulate N frames twice, compare hashes; pin the hash
  as a golden value so CI catches platform differences.
- A replay round trip through `Replay::record` and `verify`.
- A lockstep or rollback test over `LoopbackNetwork` with latency and loss.
- A "draws every phase" test that renders each phase and checks the frame
  hash changes.

`games/ippon/tests/determinism.rs` is the template.
