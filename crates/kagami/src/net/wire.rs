//! Wire format for lockstep messages. Little-endian, hand-rolled, tiny.

use crate::input::Input;

const TAG_INPUT: u8 = 1;
const TAG_HASH: u8 = 2;

/// Maximum inputs carried in one input message.
pub const MAX_WINDOW: usize = 32;

/// A lockstep protocol message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    /// A window of recent inputs from one player. `inputs[i]` is the input
    /// for frame `last_frame - (inputs.len() - 1) + i`.
    Inputs {
        /// Sending player.
        from: u8,
        /// Frame of the last input in the window.
        last_frame: u32,
        /// Inputs, oldest first.
        inputs: Vec<Input>,
    },
    /// A state hash for a frame.
    Hash {
        /// Sending player.
        from: u8,
        /// Frame the hash was taken after.
        frame: u32,
        /// The hash.
        hash: u64,
    },
}

impl Message {
    /// Serializes the message.
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Message::Inputs {
                from,
                last_frame,
                inputs,
            } => {
                let n = inputs.len().min(MAX_WINDOW);
                let mut v = Vec::with_capacity(7 + n);
                v.push(TAG_INPUT);
                v.push(*from);
                v.extend_from_slice(&last_frame.to_le_bytes());
                v.push(n as u8);
                for i in &inputs[inputs.len() - n..] {
                    v.push(i.bits());
                }
                v
            }
            Message::Hash { from, frame, hash } => {
                let mut v = Vec::with_capacity(14);
                v.push(TAG_HASH);
                v.push(*from);
                v.extend_from_slice(&frame.to_le_bytes());
                v.extend_from_slice(&hash.to_le_bytes());
                v
            }
        }
    }

    /// Parses a message. Returns `None` for anything malformed.
    pub fn decode(data: &[u8]) -> Option<Message> {
        match *data.first()? {
            TAG_INPUT => {
                if data.len() < 7 {
                    return None;
                }
                let from = data[1];
                let last_frame = u32::from_le_bytes([data[2], data[3], data[4], data[5]]);
                let n = data[6] as usize;
                if n == 0 || n > MAX_WINDOW || data.len() != 7 + n {
                    return None;
                }
                let inputs = data[7..].iter().map(|&b| Input::from_bits(b)).collect();
                Some(Message::Inputs {
                    from,
                    last_frame,
                    inputs,
                })
            }
            TAG_HASH => {
                if data.len() != 14 {
                    return None;
                }
                let from = data[1];
                let frame = u32::from_le_bytes([data[2], data[3], data[4], data[5]]);
                let mut h = [0u8; 8];
                h.copy_from_slice(&data[6..14]);
                Some(Message::Hash {
                    from,
                    frame,
                    hash: u64::from_le_bytes(h),
                })
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let m = Message::Inputs {
            from: 1,
            last_frame: 1000,
            inputs: vec![Input::A, Input::NONE, Input::LEFT | Input::B],
        };
        assert_eq!(Message::decode(&m.encode()), Some(m.clone()));
        let h = Message::Hash {
            from: 0,
            frame: 60,
            hash: 0xDEAD_BEEF_CAFE_F00D,
        };
        assert_eq!(Message::decode(&h.encode()), Some(h));
    }

    #[test]
    fn window_is_capped() {
        let inputs: Vec<Input> = (0..40).map(Input::from_bits).collect();
        let m = Message::Inputs {
            from: 0,
            last_frame: 39,
            inputs,
        };
        let Some(Message::Inputs {
            inputs, last_frame, ..
        }) = Message::decode(&m.encode())
        else {
            panic!()
        };
        assert_eq!(inputs.len(), MAX_WINDOW);
        assert_eq!(last_frame, 39);
        assert_eq!(inputs[0], Input::from_bits(8));
        assert_eq!(inputs[31], Input::from_bits(39));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(Message::decode(&[]), None);
        assert_eq!(Message::decode(&[9, 0, 0]), None);
        assert_eq!(Message::decode(&[TAG_INPUT, 0, 0, 0, 0, 0, 0]), None); // n = 0
        assert_eq!(Message::decode(&[TAG_INPUT, 0, 0, 0, 0, 0, 2, 1]), None); // short
        assert_eq!(Message::decode(&[TAG_INPUT, 0, 0, 0, 0, 0, 33]), None); // too wide
        assert_eq!(Message::decode(&[TAG_INPUT, 0, 0, 0]), None);
        assert_eq!(Message::decode(&[TAG_HASH, 0, 0]), None);
    }
}
