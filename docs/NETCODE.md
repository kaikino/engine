# Netcode

KAGAMI ships two netcodes on one wire format. **Input-delay lockstep** is
the simplest netcode that is fully correct. **Rollback** predicts and
corrects, trading a game-state copy per frame for near-zero input delay.
Both work because the console is deterministic: nothing but inputs and
hashes ever crosses the network. Pick with `--rollback`.

## The protocol

Each peer has a player index. On every 60 Hz tick a peer:

1. Drains its transport and records any inputs and hashes received.
2. Checks whether it holds every *other* player's input for the current frame *f*. If not, it stalls: nothing is simulated, the screen shows WAITING, and after `resend_after_stalls` consecutive stalls it re-sends its latest input window.
3. Otherwise it schedules its local input for frame *f + delay*, sends an input message to every peer, simulates frame *f* with everyone's inputs, hashes the state, and on checkpoint frames sends the hash.

Frames `0 .. delay` are pre-filled with "no input" for everyone by convention,
so the first frames never wait.

### Messages

| Message | Fields | Size |
|---|---|---|
| Inputs | from, last_frame, up to 32 inputs oldest-first | 7 + n bytes |
| Hash | from, frame, 64-bit hash | 14 bytes |

An input message carries the last `redundancy` (default 8) inputs, so the
loss of any single datagram is repaired by the next one. There are no acks
and no sequence numbers; late or duplicate inputs for a frame that is
already known are ignored (`entry().or_insert`), so reordering is harmless.

Bandwidth at 60 Hz with defaults: one 15-byte datagram per tick per peer,
plus a 14-byte hash twice a second. Under a kilobyte per second.

### Input delay

`delay` frames pass between pressing a button and the game reacting.
At 60 Hz, 3 frames is 50 ms, which most players do not notice. The right
value is roughly `ceil(one-way latency / 16.7 ms) + 1`. Set it with
`--delay`. All peers must agree.

### Desync detection

Every `hash_interval` frames (default 30) each peer broadcasts its state
hash. A peer compares every remote hash it receives against its own hash for
that frame, as soon as it has both. A mismatch produces a `Desync` naming the
frame and the peer. The runtime ends the match and prints the report.
Set `hash_interval` to 0 to disable (not recommended).

## Transports

`kagami::net::Transport` is two methods, `send(to, bytes)` and
`recv() -> Option<bytes>`. It may be unreliable and unordered.

- **`UdpTransport`** binds a socket and maps player indices to addresses. Send errors count as loss.
- **`LoopbackNetwork`** is an in-memory network for `n` endpoints with configurable latency, jitter and loss, driven by a seeded RNG, advanced by `tick()`. Tests and `--netplay loopback` use it. With `--netplay loopback` the runtime runs two full peers in one window: P1 keys drive peer 0, P2 keys drive peer 1, peer 0 is displayed, and both are hash-checked. It is the easiest way to feel what a given latency and loss do to a match.

Writing a transport for WebRTC, a relay server, or a serial cable is
implementing those two methods.

## Rollback

`kagami::net::Rollback` owns the game (which is why `Game: Clone`). Each
tick it:

1. Drains the transport.
2. **Reconciles:** walks the not-yet-confirmed frames in order; at the first
   frame whose real remote inputs differ from the ones it simulated with, it
   restores the snapshot taken before that frame and re-simulates up to the
   present. Re-simulated frames use a scratch sound processor so effects do
   not fire twice.
3. **Confirms:** advances the confirmed frame while every player's input is
   known, hands those frames (with their state hashes) to the caller for the
   replay, and exchanges hashes on checkpoint frames. Hashes are only ever
   compared on confirmed frames, so predictions never cause false desyncs.
4. **Simulates one more frame** using real inputs where known and, for
   anyone late, their most recent input (people mostly keep doing what they
   were doing). If it is already `max_prediction` frames ahead of the
   confirmed frame it stalls instead.

Defaults: input delay 1, prediction window 8 frames (about 130 ms of
one-way latency before any stall), hashes every 30 confirmed frames,
12 inputs of redundancy per message.

What you see on screen is the predicted present; a wrong prediction shows
as a small correction a few frames later. The replay recorded under
rollback contains only confirmed frames and verifies like any other.

Try it against lockstep on the same simulated link:

```bash
cargo run --release -p ippon -- --netplay loopback --latency 6 --jitter 3 --loss 100
cargo run --release -p ippon -- --netplay loopback --latency 6 --jitter 3 --loss 100 --rollback
```

The first stalls constantly; the second plays smoothly and the F1 overlay
counts the rollbacks (`RB`), re-simulated frames (`RESIM`) and how far
ahead of confirmation it is running (`AHEAD`).

## What neither netcode does

- **Matchmaking, NAT traversal, relays.** Peers need to reach each other's UDP port. On a LAN that just works; across the internet you need port forwarding or a relay.
- **Hiding bad connections.** On a 200 ms link, lockstep needs a delay of ~13 frames to run stall-free, and rollback needs a prediction window that large, which means visible corrections. Neither can make a bad link feel good.
- **Cheating prevention.** Every peer sees every input. A modified client can only hurt itself (it will desync).

## Reading the stats overlay

Press F1 during netplay:

```
F1234    H9a3b2c1d  60FPS
STALL 12 TX 1250 RX 1213 REJ 0
```

`STALL` counts ticks that waited for a remote input. A steadily rising
count means the delay is too low for the link. `REJ` counts malformed or
mis-addressed messages; it should stay at 0.
