// KAGAMI browser host. Plain JavaScript, no dependencies, no bundler.
// Drives a cartridge built with `kagami_web::cartridge!` at a fixed 60 Hz,
// blits the RGBA framebuffer to a canvas, plays the APU's samples through
// Web Audio, and maps the keyboard to two controllers.

const KEYS = {
  ArrowUp: [0, 1], ArrowDown: [0, 2], ArrowLeft: [0, 4], ArrowRight: [0, 8],
  KeyZ: [0, 16], KeyX: [0, 32], KeyC: [0, 64], Enter: [0, 128],
  KeyW: [1, 1], KeyS: [1, 2], KeyA: [1, 4], KeyD: [1, 8],
  KeyJ: [1, 16], KeyK: [1, 32], KeyL: [1, 64], Tab: [1, 128],
};
const FRAME_MS = 1000 / 60;
const SAMPLE_RATE = 44100;
const MAX_CATCHUP = 5;

export async function boot({ wasmUrl, canvas, seed = 1, players = 1, autostart = true, onFrame }) {
  const { instance } = await WebAssembly.instantiateStreaming(fetch(wasmUrl), {});
  const x = instance.exports;
  const mem = () => new Uint8Array(x.memory.buffer);

  x.kagami_init(seed >>> 0, players);
  const width = x.kagami_width();
  const height = x.kagami_height();
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext('2d', { alpha: false });
  const image = ctx.createImageData(width, height);
  const titleBytes = mem().subarray(x.kagami_title_ptr(), x.kagami_title_ptr() + x.kagami_title_len());
  const title = new TextDecoder().decode(titleBytes);

  const pads = [0, 0, 0, 0];
  const onKey = (down) => (e) => {
    const b = KEYS[e.code];
    if (!b) return;
    e.preventDefault();
    pads[b[0]] = down ? (pads[b[0]] | b[1]) : (pads[b[0]] & ~b[1]);
  };
  window.addEventListener('keydown', onKey(true));
  window.addEventListener('keyup', onKey(false));

  let audio = null;
  let nextAudioTime = 0;
  const enableSound = () => {
    if (audio) return;
    audio = new (window.AudioContext || window.webkitAudioContext)({ sampleRate: SAMPLE_RATE });
    nextAudioTime = audio.currentTime + 0.05;
  };
  const pushAudio = () => {
    if (!audio) return;
    const n = x.kagami_audio_len();
    const ptr = x.kagami_audio_ptr();
    const samples = new Int16Array(x.memory.buffer, ptr, n);
    const buffer = audio.createBuffer(1, n, SAMPLE_RATE);
    const ch = buffer.getChannelData(0);
    for (let i = 0; i < n; i++) ch[i] = samples[i] / 32768;
    const src = audio.createBufferSource();
    src.buffer = buffer;
    src.connect(audio.destination);
    // Never schedule in the past; if we fell behind, resync with a tiny gap.
    if (nextAudioTime < audio.currentTime) nextAudioTime = audio.currentTime + 0.02;
    src.start(nextAudioTime);
    nextAudioTime += n / SAMPLE_RATE;
  };

  // Netplay plumbing: the session lives inside the module; we move bytes.
  let net = null; // { localPlayer, send }
  const pumpOutgoing = () => {
    if (!net) return;
    let n;
    while ((n = x.kagami_net_outgoing()) > 0) {
      const to = x.kagami_net_outgoing_to();
      const bytes = new Uint8Array(x.memory.buffer, x.kagami_outbox_ptr(), n).slice();
      net.send(to, bytes);
    }
  };
  const hex64 = (hi, lo) => (hi >>> 0).toString(16).padStart(8, '0') + (lo >>> 0).toString(16).padStart(8, '0');

  const state = {
    title, frame: 0, hash: '', fps: 0, running: false, instance,
    get packedInputs() {
      if (net) return (pads[0] << (8 * net.localPlayer)) >>> 0; // P1 keys drive the local player
      return (pads[0] | (pads[1] << 8) | (pads[2] << 16) | (pads[3] << 24)) >>> 0;
    },
    enableSound,
    /** Simulate one frame with explicit packed inputs (for tests and tools). */
    step(packed = this.packedInputs) {
      x.kagami_step(packed);
      pumpOutgoing();
      pushAudio();
      this.frame = x.kagami_frame();
      this.hash = hex64(x.kagami_hash_hi(), x.kagami_hash_lo());
    },
    /** Start a netplay session. `send(to, bytes)` carries datagrams out. */
    startNet({ localPlayer, players, delay = 2, prediction = 0, send }) {
      net = { localPlayer, send };
      x.kagami_net_start(localPlayer, players, delay, prediction);
      this.frame = 0;
    },
    stopNet() { net = null; x.kagami_net_stop(); this.frame = 0; },
    get inNetplay() { return net !== null; },
    /** Hand a datagram from the network to the session. */
    receive(bytes) {
      const n = Math.min(bytes.length, 512);
      new Uint8Array(x.memory.buffer, x.kagami_inbox_ptr(), n).set(bytes.subarray(0, n));
      x.kagami_net_recv(n);
    },
    /** 'ok' | 'waiting' | 'desynced' after the last step. */
    get netStatus() { return ['ok', 'waiting', 'desynced'][x.kagami_net_status()] ?? 'ok'; },
    get desyncFrame() { return x.kagami_net_desync_frame(); },
    get netStats() {
      const s = (i) => x.kagami_net_stat(i);
      return { frames: s(0), stalls: s(1), sent: s(2), received: s(3), rollbacks: s(4), resimulated: s(5), confirmed: s(6) };
    },
    /** Hash of a confirmed frame, or null if not remembered. */
    confirmedHash(frame) {
      const hi = x.kagami_confirmed_hash_hi(frame), lo = x.kagami_confirmed_hash_lo(frame);
      return (hi === 0 && lo === 0) ? null : hex64(hi, lo);
    },
    present() {
      image.data.set(mem().subarray(x.kagami_fb_ptr(), x.kagami_fb_ptr() + x.kagami_fb_len()));
      ctx.putImageData(image, 0, 0);
    },
    reset(newSeed = seed, newPlayers = players) {
      x.kagami_init(newSeed >>> 0, newPlayers);
      this.frame = 0;
    },
    start() { if (!this.running) { this.running = true; last = performance.now(); requestAnimationFrame(loop); } },
    stop() { this.running = false; },
  };

  let last = performance.now();
  let acc = 0;
  let fpsCount = 0, fpsSince = last;
  const loop = (now) => {
    if (!state.running) return;
    acc += now - last;
    last = now;
    let steps = 0;
    while (acc >= FRAME_MS && steps < MAX_CATCHUP) { state.step(); acc -= FRAME_MS; steps++; }
    if (steps === MAX_CATCHUP) acc = 0;
    if (steps > 0) {
      state.present();
      fpsCount++;
      if (now - fpsSince >= 1000) { state.fps = fpsCount; fpsCount = 0; fpsSince = now; }
      if (onFrame) onFrame(state);
    }
    requestAnimationFrame(loop);
  };

  state.present();
  if (autostart) state.start();
  return state;
}
