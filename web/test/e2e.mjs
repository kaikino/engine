// End-to-end test of the browser runtime with the preinstalled Playwright.
//
//   node web/test/e2e.mjs <expected-native-hash-after-120-frames>
//
// Boots IPPON in headless Chromium, steps exactly 120 frames with no input,
// and checks that the state hash equals the native build's hash for the same
// run. That is the cross-target determinism proof: wasm32 and x86-64 agree
// bit for bit. Also saves a screenshot of the canvas.
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { mkdir, writeFile } from 'node:fs/promises';
import { serve } from './serve.mjs';

// Resolve Playwright through CommonJS so a globally installed copy works too.
const { chromium } = createRequire(import.meta.url)('playwright');
const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const expected = process.argv[2];
const FRAMES = 120;

const { server, port } = await serve(root);
const browser = await chromium.launch({ args: ['--no-sandbox'] });
try {
  const page = await browser.newPage({ viewport: { width: 800, height: 700 } });
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
  await page.goto(`http://127.0.0.1:${port}/index.html?cart=ippon&seed=1&players=2&autostart=0`);
  await page.waitForFunction(() => window.kagami && window.kagami.title);
  const title = await page.evaluate(() => window.kagami.title);
  const result = await page.evaluate((n) => {
    const k = window.kagami;
    for (let i = 0; i < n; i++) k.step(0);
    k.present();
    return { frame: k.frame, hash: k.hash };
  }, FRAMES);
  // Input changes the hash: press START on P1 for one frame.
  const withInput = await page.evaluate(() => { const k = window.kagami; k.step(128); return k.hash; });
  const canvasData = await page.evaluate(() => document.getElementById('screen').toDataURL('image/png'));
  await mkdir(join(root, 'dist'), { recursive: true });
  await writeFile(join(root, 'dist', 'e2e.png'), Buffer.from(canvasData.split(',')[1], 'base64'));
  // The canvas is not blank: count distinct colors.
  const colors = await page.evaluate(() => {
    const c = document.getElementById('screen').getContext('2d');
    const d = c.getImageData(0, 0, 256, 224).data;
    const set = new Set();
    for (let i = 0; i < d.length; i += 4) set.add((d[i] << 16) | (d[i + 1] << 8) | d[i + 2]);
    return set.size;
  });
  console.log(`title: ${title}`);
  console.log(`frames: ${result.frame}`);
  console.log(`wasm state hash: ${result.hash}`);
  console.log(`distinct colors on canvas: ${colors}`);
  if (errors.length) throw new Error('page errors: ' + errors.join('; '));
  if (title !== 'IPPON') throw new Error('unexpected title ' + title);
  if (result.frame !== FRAMES) throw new Error('frame count ' + result.frame);
  if (colors < 5) throw new Error('canvas looks blank');
  if (withInput === result.hash) throw new Error('input did not change state');
  if (expected && result.hash !== expected) throw new Error(`wasm hash ${result.hash} != native ${expected}`);
  console.log(expected ? 'web e2e: OK (hash matches native)' : 'web e2e: OK');
} finally {
  await browser.close();
  server.close();
}
