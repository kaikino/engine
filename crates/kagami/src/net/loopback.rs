//! An in-memory network with simulated latency, jitter and loss.

use super::Transport;
use crate::rng::Rng;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

struct Packet {
    to: u8,
    deliver_at: u64,
    data: Vec<u8>,
}

struct Inner {
    now: u64,
    in_flight: Vec<Packet>,
    inboxes: Vec<VecDeque<Vec<u8>>>,
    rng: Rng,
    latency: u32,
    jitter: u32,
    loss_permille: u32,
    sent: u64,
    dropped: u64,
}

/// A simulated network connecting up to [`crate::MAX_PLAYERS`] endpoints.
///
/// Time advances only when [`LoopbackNetwork::tick`] is called, so tests are
/// fully deterministic given the seed.
///
/// ```
/// use kagami::net::{LoopbackNetwork, Transport};
/// let net = LoopbackNetwork::new(2, 1, 0, 0, 0);
/// let mut a = net.endpoint(0);
/// let mut b = net.endpoint(1);
/// a.send(1, b"hi");
/// net.tick();
/// assert_eq!(b.recv(), Some(b"hi".to_vec()));
/// ```
#[derive(Clone)]
pub struct LoopbackNetwork {
    inner: Rc<RefCell<Inner>>,
}

impl LoopbackNetwork {
    /// Creates a network.
    ///
    /// - `latency`: ticks before a packet is deliverable.
    /// - `jitter`: extra random ticks in `0..=jitter`.
    /// - `loss_permille`: packets dropped per thousand.
    pub fn new(players: u8, seed: u32, latency: u32, jitter: u32, loss_permille: u32) -> Self {
        LoopbackNetwork {
            inner: Rc::new(RefCell::new(Inner {
                now: 0,
                in_flight: Vec::new(),
                inboxes: (0..players).map(|_| VecDeque::new()).collect(),
                rng: Rng::new(seed),
                latency,
                jitter,
                loss_permille: loss_permille.min(1000),
                sent: 0,
                dropped: 0,
            })),
        }
    }

    /// Returns the transport handle for a player.
    pub fn endpoint(&self, player: u8) -> LoopbackEndpoint {
        LoopbackEndpoint {
            net: self.inner.clone(),
            player,
        }
    }

    /// Advances simulated time by one tick, delivering packets that are due.
    pub fn tick(&self) {
        let mut inner = self.inner.borrow_mut();
        inner.now += 1;
        let now = inner.now;
        let mut i = 0;
        while i < inner.in_flight.len() {
            if inner.in_flight[i].deliver_at <= now {
                let p = inner.in_flight.swap_remove(i);
                if let Some(inbox) = inner.inboxes.get_mut(p.to as usize) {
                    inbox.push_back(p.data);
                }
            } else {
                i += 1;
            }
        }
    }

    /// Packets sent so far.
    pub fn sent(&self) -> u64 {
        self.inner.borrow().sent
    }

    /// Packets dropped so far.
    pub fn dropped(&self) -> u64 {
        self.inner.borrow().dropped
    }
}

/// One player's connection to a [`LoopbackNetwork`].
pub struct LoopbackEndpoint {
    net: Rc<RefCell<Inner>>,
    player: u8,
}

impl Transport for LoopbackEndpoint {
    fn send(&mut self, to: u8, data: &[u8]) {
        let mut inner = self.net.borrow_mut();
        inner.sent += 1;
        if inner.loss_permille > 0 && inner.rng.below(1000) < inner.loss_permille {
            inner.dropped += 1;
            return;
        }
        let jitter_max = inner.jitter;
        let jitter = if jitter_max > 0 {
            inner.rng.below(jitter_max + 1)
        } else {
            0
        };
        let deliver_at = inner.now + u64::from(inner.latency) + u64::from(jitter);
        let _ = self.player; // sender identity travels inside the message
        inner.in_flight.push(Packet {
            to,
            deliver_at,
            data: data.to_vec(),
        });
    }

    fn recv(&mut self) -> Option<Vec<u8>> {
        self.net
            .borrow_mut()
            .inboxes
            .get_mut(self.player as usize)?
            .pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latency_delays_delivery() {
        let net = LoopbackNetwork::new(2, 3, 2, 0, 0);
        let mut a = net.endpoint(0);
        let mut b = net.endpoint(1);
        a.send(1, &[1]);
        net.tick();
        assert_eq!(b.recv(), None);
        net.tick();
        assert_eq!(b.recv(), Some(vec![1]));
        assert_eq!(b.recv(), None);
        assert_eq!(net.sent(), 1);
        assert_eq!(net.dropped(), 0);
    }

    #[test]
    fn loss_drops_packets_deterministically() {
        let net = LoopbackNetwork::new(2, 42, 0, 0, 500);
        let mut a = net.endpoint(0);
        for _ in 0..1000 {
            a.send(1, &[0]);
        }
        net.tick();
        let d = net.dropped();
        assert!(d > 400 && d < 600, "{d}");
        let net2 = LoopbackNetwork::new(2, 42, 0, 0, 500);
        let mut a2 = net2.endpoint(0);
        for _ in 0..1000 {
            a2.send(1, &[0]);
        }
        assert_eq!(net2.dropped(), d);
        // Full loss.
        let net3 = LoopbackNetwork::new(2, 1, 0, 0, 5000);
        let mut a3 = net3.endpoint(0);
        a3.send(1, &[0]);
        net3.tick();
        assert_eq!(net3.endpoint(1).recv(), None);
    }

    #[test]
    fn jitter_reorders_but_delivers() {
        let net = LoopbackNetwork::new(2, 7, 1, 5, 0);
        let mut a = net.endpoint(0);
        let mut b = net.endpoint(1);
        for i in 0..20u8 {
            a.send(1, &[i]);
        }
        let mut got = Vec::new();
        for _ in 0..10 {
            net.tick();
            while let Some(p) = b.recv() {
                got.push(p[0]);
            }
        }
        let mut sorted = got.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..20).collect::<Vec<_>>());
        // Sending to an unknown player is harmless.
        a.send(9, &[0]);
        net.tick();
        assert_eq!(net.endpoint(9).recv(), None);
    }
}
