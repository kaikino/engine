//! Deterministic lockstep networking.
//!
//! KAGAMI never sends game state over the network, only inputs. Every peer
//! runs the same simulation on the same inputs and, because the simulation is
//! deterministic, every screen is a mirror of the others.
//!
//! Two schemes are provided, sharing one wire format:
//!
//! - [`Lockstep`] is **input-delay lockstep**: the input you press on frame
//!   *f* is scheduled for frame *f + delay* and sent to every peer
//!   immediately. A frame is simulated only when every player's input for it
//!   has arrived; if one is missing the console waits (the classic "lockstep
//!   stall"). With a delay of 2–4 frames and a decent connection, stalls are
//!   rare. It never needs to copy game state.
//! - [`Rollback`] **predicts** missing remote inputs (a player keeps doing
//!   what they were doing), simulates ahead, and when the real inputs arrive
//!   and differ, restores a snapshot and re-simulates. Input delay can be 0–1
//!   and stalls only happen when a peer is more than `max_prediction` frames
//!   behind. It costs one game clone per frame of prediction.
//!
//! Every `hash_interval` frames each peer sends a hash of its game state.
//! Any mismatch is reported as a [`Desync`] naming the exact frame, which is
//! the single most useful debugging tool in networked game development.
//!
//! Transports are pluggable through [`Transport`]. [`LoopbackNetwork`] is an
//! in-memory network with simulated latency, jitter and packet loss for tests
//! and local multiplayer; [`UdpTransport`] talks to real peers.

mod lockstep;
mod loopback;
mod rollback;
mod udp;
mod wire;

pub use lockstep::{Desync, Lockstep, LockstepConfig, LockstepStats, StepResult};
pub use loopback::{LoopbackEndpoint, LoopbackNetwork};
pub use rollback::{Rollback, RollbackConfig, RollbackStats};
pub use udp::UdpTransport;
pub use wire::Message;

/// A way to exchange datagrams with other players.
///
/// Delivery may be unreliable and unordered; the lockstep layer sends
/// redundant input windows to cope with loss. Messages must not be truncated
/// or merged.
pub trait Transport {
    /// Sends a datagram to a player. `to` is a player index.
    fn send(&mut self, to: u8, data: &[u8]);
    /// Receives the next pending datagram, if any. Must not block.
    fn recv(&mut self) -> Option<Vec<u8>>;
}
