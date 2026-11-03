//! IPPON's soundtrack: a loop in the *in* scale (陰音階) and the match
//! sound effects.

use kagami::apu::{Note, Row, Song, Track, Waveform};
use kagami::{Sfx, SfxStep};

const D3: u8 = 50;
const A3: u8 = 57;
const D4: u8 = 62;
const EB4: u8 = 63;
const G4: u8 = 67;
const A4: u8 = 69;
const BB4: u8 = 70;
const D5: u8 = 74;

fn on(n: u8, v: u8) -> Row {
    Row::on(n, v)
}
const fn hold() -> Row {
    Row::hold()
}
const fn off() -> Row {
    Row::off()
}

/// The dojo theme: 32 rows, looped.
pub fn dojo_theme() -> Song {
    let lead = vec![
        on(D4, 9),
        hold(),
        on(EB4, 9),
        hold(),
        on(G4, 10),
        hold(),
        hold(),
        on(A4, 9),
        on(G4, 9),
        hold(),
        on(EB4, 8),
        hold(),
        on(D4, 9),
        hold(),
        hold(),
        off(),
        on(A4, 9),
        hold(),
        on(BB4, 9),
        hold(),
        on(D5, 10),
        hold(),
        hold(),
        on(A4, 8),
        on(G4, 9),
        hold(),
        on(EB4, 8),
        hold(),
        on(D4, 9),
        hold(),
        hold(),
        off(),
    ];
    let bass: Vec<Row> = (0..32)
        .map(|i| match i % 8 {
            0 => on(D3, 10),
            4 => on(A3, 8),
            6 => on(D3, 6),
            _ => hold(),
        })
        .collect();
    let drums: Vec<Row> = (0..32)
        .map(|i| match i % 4 {
            0 => on(40, 7),
            2 => on(60, 3),
            _ => off(),
        })
        .collect();
    Song {
        frames_per_row: 8,
        tracks: vec![
            Track {
                wave: Waveform::Pulse25,
                rows: lead,
            },
            Track {
                wave: Waveform::Pulse50,
                rows: vec![
                    Row {
                        note: Note::Off,
                        vol: 0
                    };
                    32
                ],
            },
            Track {
                wave: Waveform::Triangle,
                rows: bass,
            },
            Track {
                wave: Waveform::Noise,
                rows: drums,
            },
        ],
        looped: true,
    }
}

/// Shinai swing.
pub fn swing() -> Sfx {
    Sfx::swoosh()
}

/// A strike landing: a crack plus the referee's high tone.
pub fn hit() -> Sfx {
    Sfx::hit()
}

/// The point call.
pub fn ippon_call() -> Sfx {
    Sfx::new(
        Waveform::Pulse50,
        &[
            SfxStep::new(D5, 12, 6),
            SfxStep::new(A4, 12, 6),
            SfxStep::new(D5, 12, 16),
        ],
    )
}

/// Shinai on shinai.
pub fn block() -> Sfx {
    Sfx::new(
        Waveform::Pulse12,
        &[SfxStep::new(91, 12, 2), SfxStep::new(86, 8, 3)],
    )
}

/// Menu confirm.
pub fn start() -> Sfx {
    Sfx::coin()
}

/// Footstep.
pub fn step() -> Sfx {
    Sfx::new(Waveform::Noise, &[SfxStep::new(36, 3, 2)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_is_well_formed() {
        let s = dojo_theme();
        assert_eq!(s.rows(), 32);
        assert_eq!(s.tracks.len(), 4);
        for t in &s.tracks {
            assert_eq!(t.rows.len(), 32);
        }
        assert!(s.looped);
        for fx in [swing(), hit(), ippon_call(), block(), start(), step()] {
            assert!(fx.frames() > 0);
        }
    }
}
