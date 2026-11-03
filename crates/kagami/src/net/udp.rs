//! A UDP transport for real networks.

use super::Transport;
use std::io;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};

/// Maximum datagram we will read. Lockstep messages are far smaller.
const MAX_DATAGRAM: usize = 512;

/// Sends lockstep messages over UDP to a fixed set of peers.
///
/// Player indices map to socket addresses; a peer's own index maps to no
/// address and sends to it are ignored. The socket is non-blocking.
pub struct UdpTransport {
    socket: UdpSocket,
    peers: Vec<Option<SocketAddr>>,
    buf: Vec<u8>,
    sent: u64,
    received: u64,
}

impl UdpTransport {
    /// Binds `bind` (for example `"0.0.0.0:7777"`) and resolves each entry of
    /// `peers` by player index. Use `None` for the local player.
    pub fn bind<A: ToSocketAddrs>(bind: A, peers: &[Option<&str>]) -> io::Result<UdpTransport> {
        let socket = UdpSocket::bind(bind)?;
        socket.set_nonblocking(true)?;
        let mut resolved = Vec::with_capacity(peers.len());
        for p in peers {
            match p {
                None => resolved.push(None),
                Some(addr) => {
                    let a = addr
                        .to_socket_addrs()?
                        .next()
                        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "unresolvable"))?;
                    resolved.push(Some(a));
                }
            }
        }
        Ok(UdpTransport {
            socket,
            peers: resolved,
            buf: vec![0; MAX_DATAGRAM],
            sent: 0,
            received: 0,
        })
    }

    /// The bound local address.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    /// Datagrams sent.
    pub fn sent(&self) -> u64 {
        self.sent
    }

    /// Datagrams received.
    pub fn received(&self) -> u64 {
        self.received
    }
}

impl Transport for UdpTransport {
    fn send(&mut self, to: u8, data: &[u8]) {
        if let Some(Some(addr)) = self.peers.get(to as usize) {
            // Send errors on UDP (unreachable, buffer full) are treated like
            // loss; the redundancy window recovers.
            if self.socket.send_to(data, addr).is_ok() {
                self.sent += 1;
            }
        }
    }

    fn recv(&mut self) -> Option<Vec<u8>> {
        match self.socket.recv_from(&mut self.buf) {
            Ok((n, _)) => {
                self.received += 1;
                Some(self.buf[..n].to_vec())
            }
            Err(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn two_sockets_talk() {
        let mut b = UdpTransport::bind("127.0.0.1:0", &[None, None]).unwrap();
        let b_addr = b.local_addr().unwrap().to_string();
        let mut a = UdpTransport::bind("127.0.0.1:0", &[None, Some(&b_addr)]).unwrap();
        a.send(1, b"ping");
        a.send(0, b"to self is ignored");
        a.send(7, b"unknown player ignored");
        let mut got = None;
        for _ in 0..50 {
            if let Some(d) = b.recv() {
                got = Some(d);
                break;
            }
            sleep(Duration::from_millis(10));
        }
        assert_eq!(got, Some(b"ping".to_vec()));
        assert_eq!(a.sent(), 1);
        assert_eq!(b.received(), 1);
        assert_eq!(b.recv(), None);
        b.send(0, b"no peer configured");
        assert_eq!(b.sent(), 0);
    }

    #[test]
    fn bad_peer_address_errors() {
        assert!(UdpTransport::bind("127.0.0.1:0", &[Some("not an address")]).is_err());
    }
}
