//! HANABI's soundtrack: a festival loop in the *yō* scale (陽音階) and
//! the sound effects.

use kagami::apu::{Row, Song, Track, Waveform};
use kagami::{Sfx, SfxStep};

const D4: u8 = 62;
const E4: u8 = 64;
const G4: u8 = 67;
const A4: u8 = 69;
const B4: u8 = 71;
const D5: u8 = 74;
const D3: u8 = 50;
const G3: u8 = 55;
const A3: u8 = 57;

fn on(n: u8, v: u8) -> Row {
    Row::on(n, v)
}

/// The festival theme: 32 rows, looped.
pub fn festival() -> Song {
    let lead: Vec<Row> = [
        D4, 0, E4, 0, G4, 0, A4, 0, B4, 0, A4, 0, G4, 0, E4, 0, D4, 0, G4, 0, A4, 0, D5, 0, B4, 0,
        A4, 0, G4, 0, E4, 0,
    ]
    .iter()
    .map(|&n| if n == 0 { Row::hold() } else { on(n, 9) })
    .collect();
    let counter: Vec<Row> = (0..32)
        .map(|i| match i % 8 {
            0 => on(G4, 5),
            2 | 6 => on(A4, 5),
            4 => on(B4, 5),
            _ => Row::hold(),
        })
        .collect();
    let bass: Vec<Row> = (0..32)
        .map(|i| match i % 8 {
            0 => on(D3, 11),
            3 => on(A3, 8),
            4 => on(G3, 9),
            7 => on(A3, 7),
            _ => Row::hold(),
        })
        .collect();
    let drums: Vec<Row> = (0..32)
        .map(|i| match i % 8 {
            0 | 4 => on(38, 8),
            2 | 6 => on(62, 3),
            7 => on(58, 4),
            _ => Row::off(),
        })
        .collect();
    Song {
        frames_per_row: 7,
        tracks: vec![
            Track {
                wave: Waveform::Pulse50,
                rows: lead,
            },
            Track {
                wave: Waveform::Pulse12,
                rows: counter,
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

/// Launching a shell.
pub fn shoot() -> Sfx {
    Sfx::new(
        Waveform::Pulse25,
        &[SfxStep::new(79, 6, 1), SfxStep::new(91, 4, 2)],
    )
}

/// A lantern popping.
pub fn pop() -> Sfx {
    Sfx::new(
        Waveform::Noise,
        &[SfxStep::new(70, 9, 2), SfxStep::new(50, 5, 3)],
    )
}

/// A big burst.
pub fn burst() -> Sfx {
    Sfx::new(
        Waveform::Noise,
        &[
            SfxStep::new(45, 15, 4),
            SfxStep::new(40, 11, 6),
            SfxStep::new(35, 6, 10),
        ],
    )
}

/// Losing a boat.
pub fn sink() -> Sfx {
    Sfx::new(
        Waveform::Pulse50,
        &[
            SfxStep::new(60, 12, 5),
            SfxStep::new(55, 12, 5),
            SfxStep::new(48, 12, 8),
            SfxStep::new(41, 8, 14),
        ],
    )
}

/// Bomb launch.
pub fn bomb() -> Sfx {
    Sfx::new(
        Waveform::Triangle,
        &[
            SfxStep::new(50, 15, 6),
            SfxStep::new(62, 15, 6),
            SfxStep::new(74, 15, 10),
        ],
    )
}

/// Menu confirm.
pub fn start() -> Sfx {
    Sfx::coin()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_and_effects_are_well_formed() {
        let s = festival();
        assert_eq!(s.rows(), 32);
        for t in &s.tracks {
            assert_eq!(t.rows.len(), 32);
        }
        for fx in [shoot(), pop(), burst(), sink(), bomb(), start()] {
            assert!(fx.frames() > 0);
        }
    }
}
