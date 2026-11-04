// Drives the tracker in headless Chromium: enter notes, export Rust, render
// four frames through the WebAssembly synth, and save both for the native
// song_check example to parse and compare sample-for-sample.
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { mkdir, writeFile } from 'node:fs/promises';
import { serve } from './serve.mjs';

const { chromium } = createRequire(import.meta.url)('playwright');
const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const { server, port } = await serve(root);
const browser = await chromium.launch({ args: ['--no-sandbox'] });
try {
  const page = await browser.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.goto(`http://127.0.0.1:${port}/tools/tracker.html`);
  await page.waitForFunction(() => window.tracker && window.tracker.ready);
  const result = await page.evaluate(() => {
    const t = window.tracker;
    t.setRows(8);
    t.set(0, 0, { kind: 'on', note: 62, vol: 9 });
    t.set(0, 2, { kind: 'on', note: 67, vol: 9 });
    t.set(0, 4, { kind: 'off' });
    t.set(2, 0, { kind: 'on', note: 50, vol: 11 });
    t.set(3, 0, { kind: 'on', note: 40, vol: 7 });
    t.set(3, 2, { kind: 'off' });
    const rust = t.exportRust();
    const samples = t.renderFrames(4);
    const back = t.importRust(rust);
    return { rust, samples, back, rows: t.tracks[0].rows.length, first: t.tracks[0].rows[0], wave3: t.tracks[3].wave };
  });
  await mkdir(join(root, 'dist'), { recursive: true });
  await writeFile(join(root, 'dist', 'song-export.rs'), result.rust);
  await writeFile(join(root, 'dist', 'song-samples.bin'), Buffer.from(new Int16Array(result.samples).buffer));
  if (errors.length) throw new Error(errors.join('; '));
  if (!result.back || result.rows !== 8 || result.first.kind !== 'on' || result.first.note !== 62 || result.wave3 !== 5) throw new Error('round trip changed the song');
  const energy = result.samples.reduce((a, s) => a + Math.abs(s), 0);
  if (energy === 0) throw new Error('synth produced silence');
  if (!result.rust.includes('Row::on(62, 9)') || !result.rust.includes('Waveform::Noise')) throw new Error('unexpected export');
  console.log(`tracker e2e: OK (${result.samples.length} samples, energy ${energy})`);
} finally {
  await browser.close();
  server.close();
}
