//! Parses a `song()` snippet (as produced by `web/tools/tracker.html`),
//! renders it with the native synth, and optionally compares the samples
//! against a file of little-endian i16 samples rendered elsewhere.
//!
//! ```text
//! cargo run -p kagami --example song_check -- song.rs [samples.bin]
//! ```

use kagami::apu::{Note, Row, Song, Track, Waveform, SAMPLES_PER_FRAME};
use kagami::Apu;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: song_check <song.rs> [samples.bin]");
    let text = std::fs::read_to_string(&path).expect("read song");
    let song = parse(&text).expect("no song() found");
    let mut apu = Apu::new();
    apu.play_song(song.clone());
    let frames = 4;
    let mut rendered: Vec<i16> = Vec::with_capacity(frames * SAMPLES_PER_FRAME);
    let mut buf = [0i16; SAMPLES_PER_FRAME];
    for _ in 0..frames {
        apu.render_frame(&mut buf);
        rendered.extend_from_slice(&buf);
    }
    let energy: u64 = rendered.iter().map(|s| u64::from(s.unsigned_abs())).sum();
    println!(
        "{path}: {} tracks, {} rows, {} frames/row, looped {}, energy {energy}",
        song.tracks.len(),
        song.rows(),
        song.frames_per_row,
        song.looped
    );
    if let Some(samples_path) = args.next() {
        let bytes = std::fs::read(&samples_path).expect("read samples");
        let theirs: Vec<i16> = bytes
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        let n = theirs.len().min(rendered.len());
        if n == 0 || theirs[..n] != rendered[..n] {
            eprintln!("samples differ from {samples_path} (compared {n})");
            std::process::exit(1);
        }
        println!("{n} samples identical to {samples_path}");
    }
}

fn parse(text: &str) -> Option<Song> {
    let frames_per_row: u8 = capture(text, "frames_per_row:", ",")?.trim().parse().ok()?;
    let looped = capture(text, "looped:", ",")?.trim() == "true";
    let mut tracks = Vec::new();
    for part in text.split("Track {").skip(1) {
        let wave = match capture(part, "Waveform::", ",")?.trim() {
            "Pulse12" => Waveform::Pulse12,
            "Pulse25" => Waveform::Pulse25,
            "Pulse75" => Waveform::Pulse75,
            "Triangle" => Waveform::Triangle,
            "Noise" => Waveform::Noise,
            _ => Waveform::Pulse50,
        };
        let mut rows = Vec::new();
        let mut rest = part;
        while let Some(i) = rest.find("Row::") {
            rest = &rest[i + 5..];
            let end = rest.find(')')?;
            let call = &rest[..end];
            rows.push(if let Some(args) = call.strip_prefix("on(") {
                let (n, v) = args.split_once(',')?;
                Row {
                    note: Note::On(n.trim().parse().ok()?),
                    vol: v.trim().parse().ok()?,
                }
            } else if call.starts_with("off(") {
                Row::off()
            } else {
                Row::hold()
            });
            rest = &rest[end..];
        }
        tracks.push(Track { wave, rows });
    }
    Some(Song {
        frames_per_row,
        tracks,
        looped,
    })
}

fn capture<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let i = text.find(start)? + start.len();
    let j = text[i..].find(end)? + i;
    Some(&text[i..j])
}
