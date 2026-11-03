//! The audio processing unit: a four-channel chiptune synthesizer.
//!
//! | Channel | Waveform                         |
//! |---------|----------------------------------|
//! | 0, 1    | pulse (12.5 / 25 / 50 / 75 % duty) |
//! | 2       | 32-step triangle                 |
//! | 3       | 15-bit LFSR noise                |
//!
//! Any channel may actually play any waveform; the table is the convention
//! that songs follow. Output is 44.1 kHz mono 16-bit, 735 samples per frame.
//!
//! The synth is driven entirely by frame-rate events ([`Apu::sfx`],
//! [`Apu::play_song`]) issued from `Game::update`, so audio follows the
//! deterministic timeline: a replay sounds identical to the original match.
//! The synthesizer itself uses only integer math, so even the sample stream
//! is bit-identical across platforms.

/// Output sample rate in Hz.
pub const SAMPLE_RATE: u32 = 44_100;
/// Samples generated per 60 Hz frame.
pub const SAMPLES_PER_FRAME: usize = 735;
/// Number of channels.
pub const CHANNELS: usize = 4;
/// Highest volume value for channels and master.
pub const MAX_VOLUME: u8 = 15;

/// Waveforms a channel can produce.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Waveform {
    /// Pulse with 12.5% duty cycle (thin, reedy).
    Pulse12,
    /// Pulse with 25% duty cycle.
    Pulse25,
    /// Square wave.
    #[default]
    Pulse50,
    /// Pulse with 75% duty (same timbre as 25%, phase-inverted).
    Pulse75,
    /// 32-step triangle (soft bass and leads).
    Triangle,
    /// Pseudo-random noise (drums, hits).
    Noise,
}

/// A note event in a song row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Note {
    /// Keep whatever the channel is doing.
    #[default]
    Hold,
    /// Silence the channel.
    Off,
    /// Start a MIDI note number (69 = A4 = 440 Hz).
    On(u8),
}

/// One row of one track.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Row {
    /// Note event.
    pub note: Note,
    /// Volume `0..=15` applied when the note is `On` (ignored for `Hold`).
    pub vol: u8,
}

impl Row {
    /// A row that starts a note.
    pub const fn on(note: u8, vol: u8) -> Row {
        Row {
            note: Note::On(note),
            vol,
        }
    }
    /// A row that holds the previous note.
    pub const fn hold() -> Row {
        Row {
            note: Note::Hold,
            vol: 0,
        }
    }
    /// A row that silences the channel.
    pub const fn off() -> Row {
        Row {
            note: Note::Off,
            vol: 0,
        }
    }
}

/// One channel's part of a song.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Track {
    /// Waveform for this track.
    pub wave: Waveform,
    /// Rows, played in order; all tracks in a song should have equal length.
    pub rows: Vec<Row>,
}

/// A tracker-style song.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Song {
    /// Frames each row lasts (6 = 150 BPM at 4 rows per beat).
    pub frames_per_row: u8,
    /// Up to four tracks, one per channel.
    pub tracks: Vec<Track>,
    /// Restart from the top when the last row finishes.
    pub looped: bool,
}

impl Song {
    /// Length in rows (the longest track).
    pub fn rows(&self) -> usize {
        self.tracks.iter().map(|t| t.rows.len()).max().unwrap_or(0)
    }
}

/// One step of a sound effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SfxStep {
    /// MIDI note number; `0` means rest.
    pub note: u8,
    /// Volume `0..=15`.
    pub vol: u8,
    /// Duration in frames (at least 1).
    pub frames: u8,
}

impl SfxStep {
    /// Builds a step.
    pub const fn new(note: u8, vol: u8, frames: u8) -> SfxStep {
        SfxStep { note, vol, frames }
    }
}

/// A sound effect: a waveform and a short sequence of steps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sfx {
    /// Waveform.
    pub wave: Waveform,
    /// Steps, played in order.
    pub steps: Vec<SfxStep>,
}

impl Sfx {
    /// Builds an effect.
    pub fn new(wave: Waveform, steps: &[SfxStep]) -> Sfx {
        Sfx {
            wave,
            steps: steps.to_vec(),
        }
    }
    /// A short menu blip.
    pub fn blip() -> Sfx {
        Sfx::new(
            Waveform::Pulse50,
            &[SfxStep::new(84, 10, 2), SfxStep::new(96, 8, 2)],
        )
    }
    /// A rising coin / pickup sound.
    pub fn coin() -> Sfx {
        Sfx::new(
            Waveform::Pulse25,
            &[SfxStep::new(83, 12, 3), SfxStep::new(88, 12, 8)],
        )
    }
    /// A percussive hit.
    pub fn hit() -> Sfx {
        Sfx::new(
            Waveform::Noise,
            &[
                SfxStep::new(60, 15, 2),
                SfxStep::new(48, 12, 3),
                SfxStep::new(40, 6, 4),
            ],
        )
    }
    /// A quick downward swoosh.
    pub fn swoosh() -> Sfx {
        Sfx::new(
            Waveform::Noise,
            &[
                SfxStep::new(90, 6, 2),
                SfxStep::new(84, 9, 2),
                SfxStep::new(78, 6, 3),
            ],
        )
    }
    /// A descending "miss" buzz.
    pub fn buzz() -> Sfx {
        Sfx::new(
            Waveform::Pulse12,
            &[SfxStep::new(45, 12, 6), SfxStep::new(41, 10, 8)],
        )
    }
    /// Total duration in frames.
    pub fn frames(&self) -> u32 {
        self.steps.iter().map(|s| u32::from(s.frames.max(1))).sum()
    }
}

/// Equal-temperament semitone ratios 2^(k/12) in 8.24 fixed point.
const SEMITONE_8_24: [u64; 12] = [
    16_777_216, 17_774_841, 18_831_729, 19_951_690, 21_137_968, 22_394_877, 23_726_566, 25_137_447,
    26_632_217, 28_215_881, 29_893_710, 31_671_577,
];

/// Frequency of a MIDI note in millihertz, computed with integer math.
pub const fn note_mhz(note: u8) -> u64 {
    let n = note as i32 - 69;
    let k = n.rem_euclid(12) as usize;
    let oct = n.div_euclid(12);
    let base = (440_000u64 * SEMITONE_8_24[k]) >> 24;
    if oct >= 0 {
        base << oct
    } else {
        base >> (-oct)
    }
}

/// Phase increment per sample for a note, in 32-bit phase units.
const fn phase_inc(note: u8) -> u32 {
    let mhz = note_mhz(note) as u128;
    ((mhz << 32) / (SAMPLE_RATE as u128 * 1000)) as u32
}

const PHASE_INC: [u32; 128] = build_phase_table();

const fn build_phase_table() -> [u32; 128] {
    let mut t = [0u32; 128];
    let mut i = 0;
    while i < 128 {
        t[i] = phase_inc(i as u8);
        i += 1;
    }
    t
}

#[derive(Clone, Debug)]
struct ActiveSfx {
    sfx: Sfx,
    step: usize,
    frames_left: u8,
}

/// One synthesizer voice.
#[derive(Clone, Debug)]
struct Channel {
    wave: Waveform,
    phase: u32,
    inc: u32,
    vol: u8,
    lfsr: u16,
    // The song's view of this channel, restored when an effect ends.
    song_wave: Waveform,
    song_inc: u32,
    song_vol: u8,
    sfx: Option<ActiveSfx>,
}

impl Default for Channel {
    fn default() -> Self {
        Channel {
            wave: Waveform::Pulse50,
            phase: 0,
            inc: 0,
            vol: 0,
            lfsr: 1,
            song_wave: Waveform::Pulse50,
            song_inc: 0,
            song_vol: 0,
            sfx: None,
        }
    }
}

impl Channel {
    fn set_note(&mut self, wave: Waveform, note: u8, vol: u8) {
        self.wave = wave;
        self.vol = vol.min(MAX_VOLUME);
        if note == 0 {
            self.vol = 0;
        } else {
            self.inc = PHASE_INC[(note as usize).min(127)];
        }
    }

    #[inline]
    fn sample(&mut self) -> i32 {
        if self.vol == 0 || self.inc == 0 {
            // Keep the phase advancing so a note that resumes stays continuous.
            self.phase = self.phase.wrapping_add(self.inc);
            return 0;
        }
        let amp = i32::from(self.vol) * 400; // 15 * 400 * 4 channels = 24000 < i16::MAX
        let out = match self.wave {
            Waveform::Pulse12 => {
                if self.phase < (1u32 << 29) {
                    amp
                } else {
                    -amp
                }
            }
            Waveform::Pulse25 => {
                if self.phase < (1u32 << 30) {
                    amp
                } else {
                    -amp
                }
            }
            Waveform::Pulse50 => {
                if self.phase < (1u32 << 31) {
                    amp
                } else {
                    -amp
                }
            }
            Waveform::Pulse75 => {
                if self.phase < (3u32 << 30) {
                    amp
                } else {
                    -amp
                }
            }
            Waveform::Triangle => {
                let step = (self.phase >> 27) as i32; // 0..32
                let tri = if step < 16 { step } else { 31 - step }; // 0..15
                (tri * 2 - 15) * amp / 15
            }
            Waveform::Noise => {
                if self.lfsr & 1 != 0 {
                    amp
                } else {
                    -amp
                }
            }
        };
        let (next, wrapped) = self.phase.overflowing_add(self.inc);
        self.phase = next;
        if wrapped && self.wave == Waveform::Noise {
            // 15-bit LFSR with taps at bits 0 and 1 (NES-style).
            let feedback = (self.lfsr ^ (self.lfsr >> 1)) & 1;
            self.lfsr = (self.lfsr >> 1) | (feedback << 14);
            if self.lfsr == 0 {
                self.lfsr = 1;
            }
        }
        out
    }
}

/// The sound processor.
#[derive(Clone, Debug)]
pub struct Apu {
    channels: Vec<Channel>,
    song: Option<Song>,
    song_row: usize,
    song_frame: u8,
    master: u8,
    frame: u32,
}

impl Default for Apu {
    fn default() -> Self {
        Self::new()
    }
}

impl Apu {
    /// Creates a silent APU.
    pub fn new() -> Apu {
        Apu {
            channels: vec![Channel::default(); CHANNELS],
            song: None,
            song_row: 0,
            song_frame: 0,
            master: MAX_VOLUME,
            frame: 0,
        }
    }

    /// Sets the master volume `0..=15`.
    pub fn set_master_volume(&mut self, vol: u8) {
        self.master = vol.min(MAX_VOLUME);
    }

    /// Current master volume.
    pub fn master_volume(&self) -> u8 {
        self.master
    }

    /// Starts a sound effect on a channel, interrupting whatever the song is
    /// doing there until the effect finishes.
    pub fn sfx(&mut self, channel: usize, sfx: &Sfx) {
        if sfx.steps.is_empty() {
            return;
        }
        let ch = &mut self.channels[channel % CHANNELS];
        let first = sfx.steps[0];
        ch.set_note(sfx.wave, first.note, first.vol);
        ch.sfx = Some(ActiveSfx {
            sfx: sfx.clone(),
            step: 0,
            frames_left: first.frames.max(1),
        });
    }

    /// Returns `true` while an effect is playing on the channel.
    pub fn sfx_active(&self, channel: usize) -> bool {
        self.channels[channel % CHANNELS].sfx.is_some()
    }

    /// Starts a song from the top.
    pub fn play_song(&mut self, song: Song) {
        self.song_row = 0;
        self.song_frame = 0;
        self.song = Some(song);
        self.apply_song_row();
    }

    /// Stops the song and silences channels not playing effects.
    pub fn stop_song(&mut self) {
        self.song = None;
        for ch in &mut self.channels {
            ch.song_vol = 0;
            if ch.sfx.is_none() {
                ch.vol = 0;
            }
        }
    }

    /// Returns `true` while a song is playing.
    pub fn song_playing(&self) -> bool {
        self.song.is_some()
    }

    /// Current song row, or `None` when no song plays.
    pub fn song_row(&self) -> Option<usize> {
        self.song.as_ref().map(|_| self.song_row)
    }

    /// Frames rendered so far.
    pub fn frame(&self) -> u32 {
        self.frame
    }

    fn apply_song_row(&mut self) {
        let Some(song) = &self.song else { return };
        for (i, track) in song.tracks.iter().enumerate().take(CHANNELS) {
            let Some(row) = track.rows.get(self.song_row) else {
                continue;
            };
            let ch = &mut self.channels[i];
            match row.note {
                Note::Hold => {}
                Note::Off => {
                    ch.song_vol = 0;
                    if ch.sfx.is_none() {
                        ch.vol = 0;
                    }
                }
                Note::On(n) => {
                    ch.song_wave = track.wave;
                    ch.song_inc = PHASE_INC[(n as usize).min(127)];
                    ch.song_vol = row.vol.min(MAX_VOLUME);
                    if ch.sfx.is_none() {
                        ch.wave = track.wave;
                        ch.inc = ch.song_inc;
                        ch.vol = ch.song_vol;
                    }
                }
            }
        }
    }

    /// Advances all sequencers by one frame. Called by [`Apu::render_frame`].
    fn tick(&mut self) {
        // Effects.
        for ch in &mut self.channels {
            let Some(active) = &mut ch.sfx else { continue };
            active.frames_left -= 1;
            if active.frames_left > 0 {
                continue;
            }
            active.step += 1;
            if let Some(step) = active.sfx.steps.get(active.step).copied() {
                let wave = active.sfx.wave;
                active.frames_left = step.frames.max(1);
                ch.set_note(wave, step.note, step.vol);
            } else {
                ch.sfx = None;
                ch.wave = ch.song_wave;
                ch.inc = ch.song_inc;
                ch.vol = ch.song_vol;
            }
        }
        // Song.
        if let Some(song) = &self.song {
            let fpr = song.frames_per_row.max(1);
            let rows = song.rows();
            let looped = song.looped;
            self.song_frame += 1;
            if self.song_frame >= fpr {
                self.song_frame = 0;
                self.song_row += 1;
                if self.song_row >= rows {
                    if looped && rows > 0 {
                        self.song_row = 0;
                    } else {
                        self.stop_song();
                        return;
                    }
                }
                self.apply_song_row();
            }
        }
    }

    /// Advances one frame and synthesizes its 735 samples into `out`.
    pub fn render_frame(&mut self, out: &mut [i16; SAMPLES_PER_FRAME]) {
        self.tick();
        self.frame = self.frame.wrapping_add(1);
        let master = i32::from(self.master);
        for s in out.iter_mut() {
            let mut mix = 0i32;
            for ch in &mut self.channels {
                mix += ch.sample();
            }
            *s = (mix * master / i32::from(MAX_VOLUME)).clamp(-32_768, 32_767) as i16;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(apu: &mut Apu) -> [i16; SAMPLES_PER_FRAME] {
        let mut buf = [0i16; SAMPLES_PER_FRAME];
        apu.render_frame(&mut buf);
        buf
    }

    fn energy(buf: &[i16]) -> u64 {
        buf.iter().map(|&s| u64::from(s.unsigned_abs())).sum()
    }

    #[test]
    fn note_frequencies() {
        assert_eq!(note_mhz(69), 440_000);
        assert_eq!(note_mhz(81), 880_000);
        assert_eq!(note_mhz(57), 220_000);
        // Middle C is 261.63 Hz.
        let c4 = note_mhz(60);
        assert!((261_000..262_500).contains(&c4), "{c4}");
        assert!(note_mhz(0) > 8_000 && note_mhz(0) < 8_300);
        assert!(note_mhz(127) > 12_500_000);
        // Phase table is monotonic.
        for i in 1..128 {
            assert!(PHASE_INC[i] > PHASE_INC[i - 1]);
        }
        // A4 should complete ~440 cycles per second: inc * 44100 ≈ 440 * 2^32.
        let cycles = u64::from(PHASE_INC[69]) * 44_100 / (1u64 << 32);
        assert_eq!(cycles, 439);
    }

    #[test]
    fn silent_by_default_and_deterministic() {
        let mut a = Apu::new();
        let mut b = Apu::new();
        assert_eq!(energy(&frame(&mut a)), 0);
        a.sfx(0, &Sfx::blip());
        b.sfx(0, &Sfx::blip());
        for _ in 0..6 {
            assert_eq!(frame(&mut a), frame(&mut b));
        }
        assert_eq!(a.frame(), 7);
        assert!(!Apu::default().song_playing());
    }

    #[test]
    fn every_waveform_makes_sound() {
        for wave in [
            Waveform::Pulse12,
            Waveform::Pulse25,
            Waveform::Pulse50,
            Waveform::Pulse75,
            Waveform::Triangle,
            Waveform::Noise,
        ] {
            let mut apu = Apu::new();
            apu.sfx(0, &Sfx::new(wave, &[SfxStep::new(60, 15, 10)]));
            let e = energy(&frame(&mut apu));
            assert!(e > 100_000, "{wave:?} energy {e}");
            // Output is bounded.
            let buf = frame(&mut apu);
            assert!(buf.iter().all(|&s| s.abs() <= 6000));
        }
    }

    #[test]
    fn sfx_steps_and_restore_song() {
        let mut apu = Apu::new();
        let song = Song {
            frames_per_row: 4,
            tracks: vec![Track {
                wave: Waveform::Triangle,
                rows: vec![Row::on(48, 8)],
            }],
            looped: true,
        };
        apu.play_song(song);
        let song_frame = frame(&mut apu);
        let fx = Sfx::new(
            Waveform::Pulse50,
            &[SfxStep::new(72, 15, 2), SfxStep::new(0, 15, 2)],
        );
        assert_eq!(fx.frames(), 4);
        apu.sfx(0, &fx);
        assert!(apu.sfx_active(0));
        let f1 = frame(&mut apu);
        assert_ne!(f1, song_frame);
        let _ = frame(&mut apu);
        // Rest step: silent.
        let f3 = frame(&mut apu);
        assert_eq!(energy(&f3), 0);
        let _ = frame(&mut apu);
        assert!(!apu.sfx_active(0));
        // Song voice returns.
        let f5 = frame(&mut apu);
        assert!(energy(&f5) > 0);
        // Empty sfx is ignored, channel index wraps.
        apu.sfx(4, &Sfx::new(Waveform::Noise, &[]));
        assert!(!apu.sfx_active(0));
        apu.sfx(4, &Sfx::hit());
        assert!(apu.sfx_active(0));
    }

    #[test]
    fn song_sequencing_loops_and_stops() {
        let rows = vec![Row::on(60, 10), Row::hold(), Row::off(), Row::on(64, 10)];
        let mk = |looped| Song {
            frames_per_row: 2,
            tracks: vec![Track {
                wave: Waveform::Pulse50,
                rows: rows.clone(),
            }],
            looped,
        };
        let mut apu = Apu::new();
        apu.play_song(mk(true));
        assert_eq!(apu.song_row(), Some(0));
        let _ = frame(&mut apu); // frame 1 of row 0
        let _ = frame(&mut apu); // -> row 1 (hold)
        assert_eq!(apu.song_row(), Some(1));
        let _ = frame(&mut apu);
        let _ = frame(&mut apu); // -> row 2 (off)
        assert_eq!(apu.song_row(), Some(2));
        assert_eq!(energy(&frame(&mut apu)), 0);
        let _ = frame(&mut apu); // -> row 3
        assert!(energy(&frame(&mut apu)) > 0);
        let _ = frame(&mut apu); // -> wraps to row 0
        assert_eq!(apu.song_row(), Some(0));
        assert!(apu.song_playing());

        let mut apu = Apu::new();
        apu.play_song(mk(false));
        for _ in 0..8 {
            let _ = frame(&mut apu);
        }
        assert!(!apu.song_playing());
        assert_eq!(apu.song_row(), None);
        assert_eq!(energy(&frame(&mut apu)), 0);
        apu.stop_song();
        assert_eq!(
            Song {
                frames_per_row: 1,
                tracks: vec![],
                looped: false
            }
            .rows(),
            0
        );
    }

    #[test]
    fn stop_song_keeps_active_sfx() {
        let mut apu = Apu::new();
        apu.play_song(Song {
            frames_per_row: 8,
            tracks: vec![
                Track {
                    wave: Waveform::Pulse50,
                    rows: vec![Row::on(60, 10)],
                },
                Track {
                    wave: Waveform::Pulse50,
                    rows: vec![Row::on(67, 10)],
                },
            ],
            looped: true,
        });
        apu.sfx(0, &Sfx::new(Waveform::Noise, &[SfxStep::new(60, 15, 30)]));
        apu.stop_song();
        assert!(apu.sfx_active(0));
        assert!(energy(&frame(&mut apu)) > 0);
    }

    #[test]
    fn master_volume_scales_and_clamps() {
        let mut loud = Apu::new();
        let mut quiet = Apu::new();
        loud.sfx(0, &Sfx::coin());
        quiet.sfx(0, &Sfx::coin());
        quiet.set_master_volume(5);
        assert_eq!(quiet.master_volume(), 5);
        let l = energy(&frame(&mut loud));
        let q = energy(&frame(&mut quiet));
        assert!(q * 3 <= l + 1000 && q * 3 >= l - 1000, "{l} {q}");
        quiet.set_master_volume(99);
        assert_eq!(quiet.master_volume(), MAX_VOLUME);
        quiet.set_master_volume(0);
        assert_eq!(energy(&frame(&mut quiet)), 0);
    }

    #[test]
    fn presets_are_well_formed() {
        for s in [
            Sfx::blip(),
            Sfx::coin(),
            Sfx::hit(),
            Sfx::swoosh(),
            Sfx::buzz(),
        ] {
            assert!(s.frames() > 0);
            assert!(s.steps.iter().all(|st| st.vol <= MAX_VOLUME));
        }
        assert_eq!(Row::default(), Row::hold());
        assert_eq!(Note::default(), Note::Hold);
        assert_eq!(Waveform::default(), Waveform::Pulse50);
        assert_eq!(Track::default().rows.len(), 0);
    }

    #[test]
    fn four_channel_mix_stays_in_range() {
        let mut apu = Apu::new();
        for c in 0..CHANNELS {
            apu.sfx(
                c,
                &Sfx::new(Waveform::Pulse50, &[SfxStep::new(60 + c as u8, 15, 20)]),
            );
        }
        let buf = frame(&mut apu);
        assert!(buf.iter().all(|&s| s.abs() <= 24_000));
        assert!(energy(&buf) > 0);
    }
}
