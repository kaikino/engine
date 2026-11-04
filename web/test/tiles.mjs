// Drives the tile editor in headless Chromium: paint, export Rust, import
// it back, and write the export for the native art_check example.
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
  await page.goto(`http://127.0.0.1:${port}/tools/tiles.html`);
  await page.waitForFunction(() => window.tiles);
  const result = await page.evaluate(() => {
    const t = window.tiles;
    t.resize(2, 1);
    // A 16x8 strip: red box outline with a gold dot inside.
    for (let x = 0; x < 16; x++) { t.set(x, 0, 10); t.set(x, 7, 10); }
    for (let y = 0; y < 8; y++) { t.set(0, y, 10); t.set(15, y, 10); }
    t.set(7, 3, 30); t.set(8, 4, 30);
    const rust = t.exportRust();
    const back = t.importRust(rust);
    return { rust, back, size: t.size, corner: t.get(0, 0), dot: t.get(7, 3), empty: t.get(3, 3) };
  });
  await mkdir(join(root, 'dist'), { recursive: true });
  await writeFile(join(root, 'dist', 'tiles-export.rs'), result.rust);
  if (errors.length) throw new Error(errors.join('; '));
  if (!result.back || result.size[0] !== 2 || result.size[1] !== 1) throw new Error('import failed');
  if (result.corner !== 10 || result.dot !== 30 || result.empty !== 0) throw new Error('round trip changed pixels');
  if (!result.rust.includes('const KEY: &str = ".au"') || !result.rust.includes('pal::BENI, pal::KIN')) throw new Error('unexpected export: ' + result.rust);
  console.log('tile editor e2e: OK');
} finally {
  await browser.close();
  server.close();
}
