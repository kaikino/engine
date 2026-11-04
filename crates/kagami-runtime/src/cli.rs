//! Command-line options. Hand-rolled: the runtime has no dependency budget to
//! spend on an argument parser.

use kagami::MAX_PLAYERS;
use std::fmt;

/// The built-in help text.
pub const USAGE: &str = "\
KAGAMI cartridge runtime

USAGE:
  <cartridge> [OPTIONS]

PLAY (default):
  --players N          local players 1..=4 on one keyboard (default: cartridge's choice)
  --seed S             match seed (default: 1)
  --scale N            window scale factor 1..=8 (default: 3)
  --mute               no audio
  --delay D            netplay input delay in frames (default: 3 lockstep, 1 rollback)
  --rollback           netplay with rollback: predict remote inputs, correct later
  --prediction N       rollback: max frames to run ahead of confirmed input (default: 8)
  --record FILE        save the match as a replay when the window closes
  --replay FILE        play back a replay instead of reading the keyboard
  --exit-after N       close the window after N frames (for smoke tests)
  --sram FILE          save file (default: kagami-<title>.sram in the working dir)
  --no-sram            do not load or save cartridge save data
  --no-gamepad         ignore gamepads

NETPLAY:
  --netplay udp --player I --bind ADDR --peer ADDR[,ADDR...]
                       lockstep over UDP. ADDRs are listed by player index,
                       skipping your own. Example (2 players):
                         host:   --netplay udp --player 0 --bind 0.0.0.0:7777 --peer 192.168.1.5:7777
                         guest:  --netplay udp --player 1 --bind 0.0.0.0:7777 --peer 192.168.1.4:7777
  --netplay loopback [--latency F] [--jitter F] [--loss PERMILLE]
                       two lockstep peers in one process on a simulated
                       network; P1 and P2 keys drive them. Demonstrates and
                       stress-tests lockstep without a second machine.

HEADLESS:
  --headless --frames N [--screenshot FILE] [--record FILE] [--random-inputs]
                       simulate without a window, print the state hash
  --verify FILE        re-simulate a replay and check every checkpoint
  --replay FILE        with --headless: simulate that replay's inputs

KEYS:
  P1: arrows, Z=A X=B C=C Enter=Start   P2: WASD, J=A K=B L=C Tab=Start
  F1 stats overlay   F2 screenshot   F3 mute   F4 save replay   Esc quit

  -h, --help           this text
";

/// Which network to use for netplay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetplayTransport {
    /// Real UDP sockets.
    Udp {
        /// Local player index.
        player: u8,
        /// Local bind address.
        bind: String,
        /// Peer addresses in player order, skipping the local player.
        peers: Vec<String>,
    },
    /// Two peers in-process on a simulated network.
    Loopback {
        /// Simulated latency in frames.
        latency: u32,
        /// Simulated jitter in frames.
        jitter: u32,
        /// Simulated packet loss per thousand.
        loss_permille: u32,
    },
}

/// What the runtime should do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Open a window and play.
    Window,
    /// Simulate without a window.
    Headless {
        /// Frames to simulate.
        frames: u32,
        /// PNG to write after the last frame.
        screenshot: Option<String>,
        /// Drive the game with seeded pseudo-random inputs.
        random_inputs: bool,
    },
    /// Verify a replay file.
    Verify {
        /// Path to the replay.
        path: String,
    },
}

/// Parsed command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// What to do.
    pub mode: Mode,
    /// Local player count; `None` means the cartridge decides.
    pub players: Option<u8>,
    /// Match seed.
    pub seed: u32,
    /// Window scale.
    pub scale: u32,
    /// Disable audio.
    pub mute: bool,
    /// Netplay input delay, if given (defaults depend on the netcode).
    pub delay: Option<u8>,
    /// Use rollback instead of lockstep for netplay.
    pub rollback: bool,
    /// Rollback prediction window in frames.
    pub prediction: u8,
    /// Replay to save.
    pub record: Option<String>,
    /// Replay to play.
    pub replay: Option<String>,
    /// Netplay configuration.
    pub netplay: Option<NetplayTransport>,
    /// Close the window after this many frames.
    pub exit_after: Option<u32>,
    /// Save file path override.
    pub sram: Option<String>,
    /// Disable save data entirely.
    pub no_sram: bool,
    /// Ignore gamepads.
    pub no_gamepad: bool,
}

impl Options {
    /// The input delay to use: the explicit `--delay`, else 1 for rollback
    /// and 3 for lockstep.
    pub fn effective_delay(&self) -> u8 {
        self.delay.unwrap_or(if self.rollback { 1 } else { 3 })
    }
}

impl Default for Options {
    fn default() -> Self {
        Options {
            mode: Mode::Window,
            players: None,
            seed: 1,
            scale: 3,
            mute: false,
            delay: None,
            rollback: false,
            prediction: 8,
            record: None,
            replay: None,
            netplay: None,
            exit_after: None,
            sram: None,
            no_sram: false,
            no_gamepad: false,
        }
    }
}

/// A command-line problem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UsageError {
    /// `--help` was requested.
    Help,
    /// An option nobody knows.
    Unknown(String),
    /// An option that needs a value was given none.
    MissingValue(String),
    /// A value that could not be parsed or is out of range.
    BadValue(String, String),
    /// Options that do not go together.
    Conflict(String),
}

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UsageError::Help => write!(f, "help requested"),
            UsageError::Unknown(o) => write!(f, "unknown option {o} (try --help)"),
            UsageError::MissingValue(o) => write!(f, "{o} needs a value"),
            UsageError::BadValue(o, v) => write!(f, "bad value {v:?} for {o}"),
            UsageError::Conflict(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for UsageError {}

fn parse_num<T: std::str::FromStr>(opt: &str, v: &str) -> Result<T, UsageError> {
    v.parse()
        .map_err(|_| UsageError::BadValue(opt.to_string(), v.to_string()))
}

/// Parses arguments (without the program name).
///
/// # Errors
/// Returns a [`UsageError`] describing the first problem found.
pub fn parse_args(args: &[String]) -> Result<Options, UsageError> {
    let mut o = Options::default();
    let mut headless = false;
    let mut frames: Option<u32> = None;
    let mut screenshot = None;
    let mut random_inputs = false;
    let mut verify = None;
    let mut netplay_kind: Option<String> = None;
    let mut player = 0u8;
    let mut bind = "0.0.0.0:7777".to_string();
    let mut peers: Vec<String> = Vec::new();
    let mut latency = 3u32;
    let mut jitter = 0u32;
    let mut loss = 0u32;

    let mut i = 0;
    let next = |i: &mut usize, name: &str| -> Result<String, UsageError> {
        *i += 1;
        args.get(*i)
            .cloned()
            .ok_or_else(|| UsageError::MissingValue(name.to_string()))
    };
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-h" | "--help" => return Err(UsageError::Help),
            "--headless" => headless = true,
            "--mute" => o.mute = true,
            "--random-inputs" => random_inputs = true,
            "--frames" => frames = Some(parse_num(a, &next(&mut i, a)?)?),
            "--screenshot" => screenshot = Some(next(&mut i, a)?),
            "--verify" => verify = Some(next(&mut i, a)?),
            "--record" => o.record = Some(next(&mut i, a)?),
            "--replay" => o.replay = Some(next(&mut i, a)?),
            "--seed" => o.seed = parse_num(a, &next(&mut i, a)?)?,
            "--exit-after" => o.exit_after = Some(parse_num(a, &next(&mut i, a)?)?),
            "--sram" => o.sram = Some(next(&mut i, a)?),
            "--no-sram" => o.no_sram = true,
            "--no-gamepad" => o.no_gamepad = true,
            "--players" => {
                let p: u8 = parse_num(a, &next(&mut i, a)?)?;
                if p == 0 || p as usize > MAX_PLAYERS {
                    return Err(UsageError::BadValue(a.to_string(), p.to_string()));
                }
                o.players = Some(p);
            }
            "--scale" => {
                let s: u32 = parse_num(a, &next(&mut i, a)?)?;
                if !(1..=8).contains(&s) {
                    return Err(UsageError::BadValue(a.to_string(), s.to_string()));
                }
                o.scale = s;
            }
            "--delay" => {
                let d: u8 = parse_num(a, &next(&mut i, a)?)?;
                if d > 32 {
                    return Err(UsageError::BadValue(a.to_string(), d.to_string()));
                }
                o.delay = Some(d);
            }
            "--rollback" => o.rollback = true,
            "--prediction" => {
                let n: u8 = parse_num(a, &next(&mut i, a)?)?;
                if !(1..=32).contains(&n) {
                    return Err(UsageError::BadValue(a.to_string(), n.to_string()));
                }
                o.prediction = n;
            }
            "--netplay" => netplay_kind = Some(next(&mut i, a)?),
            "--player" => player = parse_num(a, &next(&mut i, a)?)?,
            "--bind" => bind = next(&mut i, a)?,
            "--peer" => peers.extend(next(&mut i, a)?.split(',').map(str::to_string)),
            "--latency" => latency = parse_num(a, &next(&mut i, a)?)?,
            "--jitter" => jitter = parse_num(a, &next(&mut i, a)?)?,
            "--loss" => loss = parse_num(a, &next(&mut i, a)?)?,
            other => return Err(UsageError::Unknown(other.to_string())),
        }
        i += 1;
    }

    if let Some(kind) = netplay_kind {
        o.netplay = Some(match kind.as_str() {
            "udp" => {
                if peers.is_empty() {
                    return Err(UsageError::MissingValue("--peer".to_string()));
                }
                let players = o.players.unwrap_or(peers.len() as u8 + 1);
                if player >= players {
                    return Err(UsageError::BadValue(
                        "--player".to_string(),
                        player.to_string(),
                    ));
                }
                if usize::from(players) != peers.len() + 1 {
                    return Err(UsageError::Conflict(
                        "--players must equal the number of --peer addresses plus one".to_string(),
                    ));
                }
                o.players = Some(players);
                NetplayTransport::Udp {
                    player,
                    bind,
                    peers,
                }
            }
            "loopback" => {
                o.players = Some(2);
                NetplayTransport::Loopback {
                    latency,
                    jitter,
                    loss_permille: loss,
                }
            }
            other => {
                return Err(UsageError::BadValue(
                    "--netplay".to_string(),
                    other.to_string(),
                ))
            }
        });
    }

    o.mode = if let Some(path) = verify {
        if headless || o.netplay.is_some() {
            return Err(UsageError::Conflict("--verify stands alone".to_string()));
        }
        Mode::Verify { path }
    } else if headless {
        if o.netplay.is_some() {
            return Err(UsageError::Conflict(
                "--headless cannot be combined with --netplay".to_string(),
            ));
        }
        let frames = match (frames, &o.replay) {
            (Some(f), _) => f,
            (None, Some(_)) => 0, // replay length
            (None, None) => return Err(UsageError::MissingValue("--frames".to_string())),
        };
        Mode::Headless {
            frames,
            screenshot,
            random_inputs,
        }
    } else {
        if frames.is_some() || screenshot.is_some() || random_inputs {
            return Err(UsageError::Conflict(
                "--frames, --screenshot and --random-inputs need --headless".to_string(),
            ));
        }
        Mode::Window
    };
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Result<Options, UsageError> {
        let v: Vec<String> = s.split_whitespace().map(str::to_string).collect();
        parse_args(&v)
    }

    #[test]
    fn defaults_and_window() {
        let o = p("").unwrap();
        assert_eq!(o, Options::default());
        assert_eq!(o.mode, Mode::Window);
        let o =
            p("--players 2 --seed 7 --scale 4 --mute --delay 5 --record r.kgmr --exit-after 10")
                .unwrap();
        assert_eq!(o.players, Some(2));
        assert_eq!(o.seed, 7);
        assert_eq!(o.scale, 4);
        assert!(o.mute);
        assert_eq!(o.delay, Some(5));
        assert_eq!(o.effective_delay(), 5);
        assert_eq!(Options::default().effective_delay(), 3);
        let r = p("--rollback --prediction 12").unwrap();
        assert!(r.rollback);
        assert_eq!(r.prediction, 12);
        assert_eq!(r.effective_delay(), 1);
        assert!(matches!(
            p("--prediction 0"),
            Err(UsageError::BadValue(_, _))
        ));
        assert_eq!(o.record.as_deref(), Some("r.kgmr"));
        assert_eq!(o.exit_after, Some(10));
        let s = p("--sram saves/x.sram --no-gamepad").unwrap();
        assert_eq!(s.sram.as_deref(), Some("saves/x.sram"));
        assert!(s.no_gamepad && !s.no_sram);
        assert!(p("--no-sram").unwrap().no_sram);
    }

    #[test]
    fn headless_and_verify() {
        let o = p("--headless --frames 600 --screenshot out.png --random-inputs").unwrap();
        assert_eq!(
            o.mode,
            Mode::Headless {
                frames: 600,
                screenshot: Some("out.png".into()),
                random_inputs: true
            }
        );
        let o = p("--headless --replay m.kgmr").unwrap();
        assert!(matches!(o.mode, Mode::Headless { frames: 0, .. }));
        let o = p("--verify m.kgmr").unwrap();
        assert_eq!(
            o.mode,
            Mode::Verify {
                path: "m.kgmr".into()
            }
        );
        assert_eq!(
            p("--headless"),
            Err(UsageError::MissingValue("--frames".into()))
        );
        assert!(matches!(p("--frames 5"), Err(UsageError::Conflict(_))));
        assert!(matches!(
            p("--verify x --headless"),
            Err(UsageError::Conflict(_))
        ));
        assert!(matches!(
            p("--headless --frames 1 --netplay loopback"),
            Err(UsageError::Conflict(_))
        ));
    }

    #[test]
    fn netplay() {
        let o = p("--netplay udp --player 1 --bind 0.0.0.0:1 --peer a:1").unwrap();
        assert_eq!(
            o.netplay,
            Some(NetplayTransport::Udp {
                player: 1,
                bind: "0.0.0.0:1".into(),
                peers: vec!["a:1".into()]
            })
        );
        assert_eq!(o.players, Some(2));
        let o = p("--netplay udp --player 0 --peer a:1,b:2 --players 3").unwrap();
        assert_eq!(o.players, Some(3));
        assert!(matches!(
            p("--netplay udp --player 0 --peer a:1 --players 3"),
            Err(UsageError::Conflict(_))
        ));
        assert!(matches!(
            p("--netplay udp --player 5 --peer a:1"),
            Err(UsageError::BadValue(_, _))
        ));
        assert_eq!(
            p("--netplay udp"),
            Err(UsageError::MissingValue("--peer".into()))
        );
        let o = p("--netplay loopback --latency 4 --jitter 2 --loss 50").unwrap();
        assert_eq!(
            o.netplay,
            Some(NetplayTransport::Loopback {
                latency: 4,
                jitter: 2,
                loss_permille: 50
            })
        );
        assert!(matches!(
            p("--netplay tcp"),
            Err(UsageError::BadValue(_, _))
        ));
    }

    #[test]
    fn errors() {
        assert_eq!(p("--help"), Err(UsageError::Help));
        assert_eq!(p("-h"), Err(UsageError::Help));
        assert_eq!(p("--bogus"), Err(UsageError::Unknown("--bogus".into())));
        assert_eq!(p("--seed"), Err(UsageError::MissingValue("--seed".into())));
        assert!(matches!(p("--seed x"), Err(UsageError::BadValue(_, _))));
        assert!(matches!(p("--players 0"), Err(UsageError::BadValue(_, _))));
        assert!(matches!(p("--players 5"), Err(UsageError::BadValue(_, _))));
        assert!(matches!(p("--scale 9"), Err(UsageError::BadValue(_, _))));
        assert!(matches!(p("--delay 40"), Err(UsageError::BadValue(_, _))));
        for e in [
            UsageError::Help,
            UsageError::Unknown("x".into()),
            UsageError::MissingValue("x".into()),
            UsageError::BadValue("x".into(), "y".into()),
            UsageError::Conflict("x".into()),
        ] {
            assert_ne!(e.to_string(), "");
        }
        assert!(USAGE.contains("--headless"));
    }
}
