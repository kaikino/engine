// Two browser pages play IPPON against each other over WebRTC, with the
// test relaying the offer and answer codes (the role a chat message plays
// for humans). Both lockstep and rollback must finish with identical
// confirmed state and no desync.
//
//   node web/test/netplay.mjs
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { serve } from './serve.mjs';

const { chromium } = createRequire(import.meta.url)('playwright');
const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const FRAMES = 240;

async function match(browser, port, { rollback }) {
  const ctx = await browser.newContext();
  const host = await ctx.newPage();
  const guest = await ctx.newPage();
  const errors = [];
  for (const p of [host, guest]) {
    p.on('pageerror', (e) => errors.push(String(e)));
    p.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
    await p.goto(`http://127.0.0.1:${port}/index.html?cart=ippon&seed=9&players=2&autostart=0`);
    await p.waitForFunction(() => window.kagamiNet && window.kagami);
  }
  // Signaling by hand, like two people pasting codes.
  const offer = await host.evaluate(async () => { window.__link = await window.kagamiNet.hostLink({ ice: [] }); return window.__link.code; });
  const answer = await guest.evaluate(async (code) => { window.__link = await window.kagamiNet.joinLink(code, { ice: [] }); return window.__link.code; }, offer);
  await host.evaluate(async (code) => { await window.__link.accept(code); }, answer);
  await guest.evaluate(async () => { await window.__link.opened; });
  const opts = { rollback, delay: rollback ? 1 : 2, prediction: 8 };
  for (const [page, localPlayer] of [[host, 0], [guest, 1]]) {
    await page.evaluate((o) => {
      window.kagamiNet.attachNetplay(window.kagami, window.__link, { ...o, players: 2 });
    }, { ...opts, localPlayer });
  }
  // Step both peers by hand with distinct inputs until both pass FRAMES.
  const inputs = (i, p) => (p === 0 ? (i % 10 < 5 ? 16 : 0) : (i % 7 < 3 ? 32 : 0));
  for (let i = 0; i < FRAMES * 4; i++) {
    const [fh, fg] = await Promise.all([
      host.evaluate((inp) => { const k = window.kagami; if (k.frame < 240) k.step(inp); return k.frame; }, inputs(i, 0)),
      guest.evaluate((inp) => { const k = window.kagami; if (k.frame < 240) k.step(inp << 8); return k.frame; }, inputs(i, 1)),
    ]);
    if (fh >= FRAMES && fg >= FRAMES) break;
    await new Promise((r) => setTimeout(r, 2));
  }
  const read = (page) => page.evaluate((f) => {
    const k = window.kagami;
    return { frame: k.frame, status: k.netStatus, stats: k.netStats, hashes: Array.from({ length: 8 }, (_, j) => k.confirmedHash(f - 10 + j)) };
  }, FRAMES);
  const [h, g] = await Promise.all([read(host), read(guest)]);
  await ctx.close();
  const label = rollback ? 'rollback' : 'lockstep';
  console.log(`${label}: host frame ${h.frame} ${h.status} ${JSON.stringify(h.stats)}`);
  console.log(`${label}: guest frame ${g.frame} ${g.status} ${JSON.stringify(g.stats)}`);
  if (errors.length) throw new Error(`${label}: page errors: ${errors.join('; ')}`);
  if (h.frame < FRAMES || g.frame < FRAMES) throw new Error(`${label}: peers did not reach ${FRAMES} frames`);
  if (h.status === 'desynced' || g.status === 'desynced') throw new Error(`${label}: desync`);
  const common = h.hashes.map((x, j) => [x, g.hashes[j]]).filter(([a, b]) => a && b);
  if (common.length === 0) throw new Error(`${label}: no common confirmed frame`);
  for (const [a, b] of common) if (a !== b) throw new Error(`${label}: confirmed hashes differ ${a} ${b}`);
  if (h.stats.received === 0 || g.stats.received === 0) throw new Error(`${label}: no datagrams exchanged`);
  if (rollback && h.stats.rollbacks === 0 && g.stats.rollbacks === 0) console.log('rollback: note, no mispredictions occurred on loopback (fast link)');
  console.log(`${label}: OK, ${common.length} confirmed frames agree`);
}

const { server, port } = await serve(root);
const browser = await chromium.launch({ args: ['--no-sandbox'] });
try {
  await match(browser, port, { rollback: false });
  await match(browser, port, { rollback: true });
  console.log('web netplay e2e: OK');
} finally {
  await browser.close();
  server.close();
}
