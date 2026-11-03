//! The windowed runtime: winit for the window and keyboard, softbuffer to
//! present the framebuffer, and a fixed 60 Hz simulation clock.

use crate::audio::{AudioOutput, SampleQueue};
use crate::cli::{NetplayTransport, Options};
use crate::keys::{Controllers, KeyMap};
use crate::session::{Session, SessionEvent};
use kagami::apu::SAMPLES_PER_FRAME;
use kagami::net::{LockstepConfig, UdpTransport};
use kagami::replay::Replay;
use kagami::{palette, Game, Gpu, SCREEN_H, SCREEN_W};
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

const FRAME: Duration = Duration::from_nanos(1_000_000_000 / 60);
/// Never simulate more than this many frames per redraw, so a stall does not
/// turn into a death spiral.
const MAX_CATCHUP: u32 = 5;

struct App<G: Game> {
    session: Session<G>,
    gpu: Gpu,
    keymap: KeyMap,
    controllers: Controllers,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    last: Instant,
    accumulator: Duration,
    scale: u32,
    title: String,
    show_stats: bool,
    muted: bool,
    audio: Option<SampleQueue>,
    _audio_output: Option<AudioOutput>,
    audio_buf: [i16; SAMPLES_PER_FRAME],
    exit_after: Option<u32>,
    waiting_on: Vec<u8>,
    fps_frames: u32,
    fps_since: Instant,
    fps: u32,
    messages: Vec<String>,
    record: Option<String>,
    error: Option<String>,
}

impl<G: Game> App<G> {
    fn simulate(&mut self) {
        let inputs = self.controllers.all();
        match self.session.tick(inputs) {
            SessionEvent::Advanced(_) => self.waiting_on.clear(),
            SessionEvent::Waiting(w) => self.waiting_on = w,
            SessionEvent::Desynced(d) => {
                self.error = Some(d.to_string());
            }
        }
        self.session.apu_mut().render_frame(&mut self.audio_buf);
        if let (Some(q), false) = (&self.audio, self.muted) {
            q.push(&self.audio_buf);
        }
    }

    fn draw(&mut self) {
        self.gpu.begin_frame();
        self.session.game().draw(&mut self.gpu);
        if !self.waiting_on.is_empty() {
            let who: Vec<String> = self
                .waiting_on
                .iter()
                .map(|p| format!("P{}", p + 1))
                .collect();
            let msg = format!("WAITING FOR {}", who.join(" "));
            self.gpu
                .rect_fill(0, 104, SCREEN_W as i32, 16, palette::SUMI);
            self.gpu.text_centered(108, &msg, palette::GOFUN);
        }
        if self.show_stats {
            let f = self.session.frame();
            let h = self.session.state_hash();
            self.gpu.rect_fill(0, 0, SCREEN_W as i32, 18, palette::SUMI);
            self.gpu.text(
                2,
                1,
                &format!("F{f:<7} H{:08x} {}FPS", h as u32, self.fps),
                palette::KI,
            );
            let line2 = match self.session.stats() {
                Some(s) => format!(
                    "STALL {} TX {} RX {} REJ {}",
                    s.stalls, s.sent, s.received, s.rejected
                ),
                None => format!(
                    "SPR {} DROP {}",
                    self.gpu.sprite_count(),
                    self.gpu.dropped_sprite_lines()
                ),
            };
            self.gpu.text(2, 9, &line2, palette::SORA);
        }
        if let Some(e) = &self.error {
            self.gpu
                .rect_fill(0, 96, SCREEN_W as i32, 32, palette::AKANE);
            self.gpu.text_centered(100, "DESYNC", palette::GOFUN);
            let short: String = e.chars().take(31).collect();
            self.gpu.text_centered(112, &short, palette::GOFUN);
        }
        self.gpu.render();
    }

    fn present(&mut self) {
        let (Some(window), Some(surface)) = (&self.window, &mut self.surface) else {
            return;
        };
        let size = window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return;
        };
        if surface.resize(w, h).is_err() {
            return;
        }
        let Ok(mut buf) = surface.buffer_mut() else {
            return;
        };
        let ww = size.width as usize;
        let wh = size.height as usize;
        let scale = ((ww / SCREEN_W).min(wh / SCREEN_H)).max(1);
        let ox = ww.saturating_sub(SCREEN_W * scale) / 2;
        let oy = wh.saturating_sub(SCREEN_H * scale) / 2;
        let rgba = self.gpu.rgba();
        for (i, px) in buf.iter_mut().enumerate() {
            let x = i % ww;
            let y = i / ww;
            let sx = (x.wrapping_sub(ox)) / scale;
            let sy = (y.wrapping_sub(oy)) / scale;
            *px = if x >= ox && y >= oy && sx < SCREEN_W && sy < SCREEN_H {
                let o = (sy * SCREEN_W + sx) * 4;
                (u32::from(rgba[o]) << 16) | (u32::from(rgba[o + 1]) << 8) | u32::from(rgba[o + 2])
            } else {
                0x0010_1010
            };
        }
        let _ = buf.present();
    }

    fn screenshot(&mut self) {
        let path = format!("kagami-{}.png", self.session.frame());
        match std::fs::write(&path, self.gpu.screenshot_png()) {
            Ok(()) => self.messages.push(format!("screenshot: {path}")),
            Err(e) => self.messages.push(format!("screenshot failed: {e}")),
        }
    }

    fn save_replay(&mut self, path: &str) {
        match std::fs::write(path, self.session.replay().encode()) {
            Ok(()) => self.messages.push(format!(
                "replay: {path} ({} frames)",
                self.session.replay().len()
            )),
            Err(e) => self.messages.push(format!("replay save failed: {e}")),
        }
    }
}

impl<G: Game> ApplicationHandler for App<G> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(&self.title)
            .with_inner_size(LogicalSize::new(
                (SCREEN_W as u32 * self.scale) as f64,
                (SCREEN_H as u32 * self.scale) as f64,
            ))
            .with_resizable(true);
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Rc::new(w),
            Err(e) => {
                self.error = Some(format!("window: {e}"));
                event_loop.exit();
                return;
            }
        };
        let context = match softbuffer::Context::new(window.clone()) {
            Ok(c) => c,
            Err(e) => {
                self.error = Some(format!("softbuffer: {e}"));
                event_loop.exit();
                return;
            }
        };
        match softbuffer::Surface::new(&context, window.clone()) {
            Ok(s) => self.surface = Some(s),
            Err(e) => {
                self.error = Some(format!("surface: {e}"));
                event_loop.exit();
                return;
            }
        }
        self.window = Some(window);
        self.last = Instant::now();
        self.fps_since = self.last;
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state,
                        repeat: false,
                        ..
                    },
                ..
            } => {
                let pressed = state == ElementState::Pressed;
                match code {
                    KeyCode::Escape if pressed => event_loop.exit(),
                    KeyCode::F1 if pressed => self.show_stats = !self.show_stats,
                    KeyCode::F2 if pressed => self.screenshot(),
                    KeyCode::F3 if pressed => self.muted = !self.muted,
                    KeyCode::F4 if pressed => {
                        let path = format!("kagami-{}.kgmr", self.session.replay().seed);
                        self.save_replay(&path);
                    }
                    _ => self.controllers.apply(&self.keymap, code, pressed),
                }
            }
            WindowEvent::RedrawRequested => self.present(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let now = Instant::now();
        self.accumulator += now - self.last;
        self.last = now;
        let mut stepped = false;
        let mut steps = 0;
        while self.accumulator >= FRAME && steps < MAX_CATCHUP {
            self.accumulator -= FRAME;
            self.simulate();
            stepped = true;
            steps += 1;
        }
        if steps == MAX_CATCHUP {
            self.accumulator = Duration::ZERO;
        }
        if stepped {
            self.draw();
            window.request_redraw();
            self.fps_frames += 1;
            if now - self.fps_since >= Duration::from_secs(1) {
                self.fps = self.fps_frames;
                self.fps_frames = 0;
                self.fps_since = now;
            }
            if let Some(n) = self.exit_after {
                if self.session.frame() >= n {
                    event_loop.exit();
                }
            }
            if self.session.replay_finished() && self.exit_after.is_none() {
                self.messages.push("replay finished".to_string());
                event_loop.exit();
            }
        } else {
            let wait = FRAME
                .saturating_sub(self.accumulator)
                .min(Duration::from_millis(4));
            std::thread::sleep(wait);
        }
    }
}

/// Opens a window and plays until it closes.
///
/// # Errors
/// Bad replay files, failed sockets, a desync, or a windowing failure.
pub fn run<G: Game + 'static>(
    opts: &Options,
    out: &mut dyn std::io::Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let seed = opts.seed;
    let probe = G::new(seed);
    let title = format!("KAGAMI - {}", probe.title());
    let default_players = probe.default_players();
    drop(probe);

    let session: Session<G> = match &opts.netplay {
        None => {
            let source = match &opts.replay {
                Some(p) => Some(Replay::decode(&std::fs::read(p)?)?),
                None => None,
            };
            let seed = source.as_ref().map_or(seed, |r| r.seed);
            let players = source
                .as_ref()
                .map(|r| r.players)
                .or(opts.players)
                .unwrap_or(default_players);
            Session::local(seed, players, source)
        }
        Some(NetplayTransport::Loopback {
            latency,
            jitter,
            loss_permille,
        }) => Session::loopback(seed, opts.delay, *latency, *jitter, *loss_permille),
        Some(NetplayTransport::Udp {
            player,
            bind,
            peers,
        }) => {
            let players = opts.players.unwrap_or(peers.len() as u8 + 1);
            let mut by_index: Vec<Option<&str>> = Vec::with_capacity(players as usize);
            let mut it = peers.iter();
            for p in 0..players {
                if p == *player {
                    by_index.push(None);
                } else {
                    by_index.push(it.next().map(String::as_str));
                }
            }
            let transport = UdpTransport::bind(bind.as_str(), &by_index)?;
            writeln!(
                out,
                "netplay: player {} of {} on {}",
                player + 1,
                players,
                transport.local_addr()?
            )?;
            let cfg = LockstepConfig {
                local_player: *player,
                players,
                input_delay: opts.delay,
                ..LockstepConfig::default()
            };
            Session::udp(seed, cfg, transport)
        }
    };

    let (audio, audio_output) = if opts.mute {
        (None, None)
    } else {
        match AudioOutput::start() {
            Ok(a) => (Some(a.queue()), Some(a)),
            Err(e) => {
                writeln!(out, "audio unavailable, continuing silent: {e}")?;
                (None, None)
            }
        }
    };

    let now = Instant::now();
    let mut app = App {
        session,
        gpu: Gpu::new(),
        keymap: KeyMap::default(),
        controllers: Controllers::default(),
        window: None,
        surface: None,
        last: now,
        accumulator: Duration::ZERO,
        scale: opts.scale,
        title,
        show_stats: false,
        muted: false,
        audio,
        _audio_output: audio_output,
        audio_buf: [0; SAMPLES_PER_FRAME],
        exit_after: opts.exit_after,
        waiting_on: Vec::new(),
        fps_frames: 0,
        fps_since: now,
        fps: 0,
        messages: Vec::new(),
        record: opts.record.clone(),
        error: None,
    };

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop.run_app(&mut app)?;

    if let Some(path) = app.record.take() {
        app.save_replay(&path);
    }
    for m in &app.messages {
        writeln!(out, "{m}")?;
    }
    writeln!(out, "frames: {}", app.session.frame())?;
    writeln!(out, "state hash: {:016x}", app.session.state_hash())?;
    if let Some(s) = app.session.stats() {
        writeln!(
            out,
            "lockstep: {} stalls, {} sent, {} received",
            s.stalls, s.sent, s.received
        )?;
    }
    match app.error {
        Some(e) => Err(e.into()),
        None => Ok(()),
    }
}
