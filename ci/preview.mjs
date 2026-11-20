// Opens the web build in Chromium (software WebGPU through SwiftShader), clicks
// through the interface and saves screenshots plus the browser console, so it
// can be checked without a Windows machine.
import { chromium } from 'playwright';
import fs from 'fs';

const url = process.argv[2] || 'http://localhost:8080/';
const out = process.argv[3] || 'preview';
fs.mkdirSync(out, { recursive: true });
const log = [];
const args = ['--enable-unsafe-webgpu', '--enable-features=Vulkan,WebGPU', '--use-vulkan=swiftshader', '--use-angle=swiftshader',
  '--use-webgpu-adapter=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist'];
const browser = await chromium.launch({ args });
const page = await browser.newPage({ viewport: { width: 1600, height: 960 }, deviceScaleFactor: 1 });
page.on('console', m => log.push(`[${m.type()}] ${m.text()}`));
page.on('pageerror', e => log.push(`[pageerror] ${e.message}`));
const wait = ms => page.waitForTimeout(ms);
const shot = async name => {
  try {
    await wait(300);
    await page.screenshot({ path: `${out}/${name}.png`, timeout: 120000 });
    log.push(`--- ${name}`);
  } catch (e) {
    log.push(`!!! ${name}: ${e.message.split('\n')[0]}`);
  }
};
const playing = async () => ((await page.locator('button.play').textContent().catch(() => '')) || '').includes('Pause');
// Pause before each screenshot: software WebGPU is too busy simulating to also capture.
const pause = async () => { if (await playing()) { await page.locator('button.play').click(); } await wait(1500); };
const play = async () => { if (!(await playing())) { await page.locator('button.play').click(); } };
const frame = async () => (await page.locator('.stats span').first().textContent().catch(() => '')) || '';
const preset = async name => { await page.selectOption('select.preset', name); };
// Software WebGPU is slow: wait until the sim has advanced `n` frames (or give up).
const untilFrame = async (n, ms) => {
  const t0 = Date.now();
  while (Date.now() - t0 < ms) {
    const f = parseInt((await frame()).replace(/\D+/g, '') || '0', 10);
    if (f >= n) return f;
    await wait(500);
  }
  return -1;
};

try {
await page.goto(url);
log.push(`frame after load: ${await untilFrame(20, 90000)}`);
await pause();
await shot('01-campfire');

await preset('Dam Break');
await play();
log.push(`dam break: ${await untilFrame(25, 120000)}`);
await pause();
await shot('02-dam-break');

await preset('Fire & Water');
await play();
log.push(`fire & water: ${await untilFrame(40, 150000)}`);
await pause();
await shot('03-fire-and-water');

// Orbit the camera, then pause.
const vp = await page.locator('.viewport canvas').boundingBox();
await page.mouse.move(vp.x + vp.width * 0.5, vp.y + vp.height * 0.5);
await page.mouse.down();
await page.mouse.move(vp.x + vp.width * 0.65, vp.y + vp.height * 0.42, { steps: 8 });
await page.mouse.up();
await wait(1500);
await shot('04-orbit-paused');

// Sparks & embers: Forge (fire + spark bursts) and Grinder Sparks, then the panel section.
await preset('Forge');
await play();
log.push(`forge: ${await untilFrame(30, 120000)}`);
await pause();
await shot('04a-forge');
await preset('Grinder Sparks');
await play();
log.push(`grinder: ${await untilFrame(30, 60000)}`);
await pause();
await page.locator('.section header', { hasText: 'Sparks' }).scrollIntoViewIfNeeded();
await shot('04b-grinder-and-panel');
// Open ocean: procedural sea to the horizon, then its panel section.
await preset('Open Ocean');
await play();
log.push(`ocean: ${await untilFrame(10, 60000)}`);
await pause();
await page.locator('.section header', { hasText: 'Ocean' }).click();
await page.locator('.section header', { hasText: 'Ocean' }).scrollIntoViewIfNeeded();
await shot('04c-open-ocean');
await preset('Fire & Water');

// Dropdowns: open Combustion (fire) and Whitewater (water), close Emitter.
await page.locator('.fold-head', { hasText: 'Emitter' }).first().click();
await page.locator('.fold-head', { hasText: 'Combustion' }).first().click();
await shot('05-fire-dropdowns');
await page.locator('.fold-head', { hasText: 'Whitewater' }).first().click();
await page.locator('.section header', { hasText: 'Water' }).scrollIntoViewIfNeeded();
await shot('06-water-dropdowns');

// Export dialog.
await page.locator('button', { hasText: 'Export' }).first().click();
await shot('07-export-dialog');

// Video export, end to end on the interface side: a stand-in for the desktop
// shell records the calls, so the frames sent to ffmpeg can be checked.
await page.evaluate(() => {
  const calls = [];
  window.__exportCalls = calls;
  window.__TAURI__ = { core: { invoke: async (cmd, args, opts) => {
    if (args instanceof Uint8Array) {
      calls.push({ cmd, bytes: args.length, id: opts?.headers?.['x-video-id'] });
    } else {
      // Tauri turns the Maps that serde-wasm-bindgen makes into plain objects; so does this
      const plain = v => v instanceof Map ? Object.fromEntries([...v].map(([k, x]) => [k, plain(x)])) : v;
      args = plain(args ?? {});
      calls.push({ cmd, args: JSON.parse(JSON.stringify(args)) });
    }
    switch (cmd) {
      case 'ffmpeg_status': return 'C:/Tidefire/ffmpeg.exe';
      case 'pick_video_file': return `C:/renders/${args.suggested}.${args.ext}`;
      case 'video_begin': return 7;
      case 'video_finish': return calls.filter(c => c.cmd === 'video_frame').length;
      default: return null;
    }
  } } };
});
const field = label => page.locator('.dialog .row', { hasText: label }).locator('input').first();
for (const [label, v] of [['Width', '320'], ['Height', '180'], ['Frames', '3']]) {
  await field(label).fill(v);
  await field(label).press('Enter');
}
await page.locator('.dialog .seg', { hasText: 'ProRes' }).click();
await page.locator('.dialog select').selectOption({ label: 'ProRes 4444 (keeps alpha)' });
await shot('07a-export-prores');
await page.locator('.dialog .seg', { hasText: 'MP4' }).click();
await page.locator('.dialog button', { hasText: 'Choose' }).click();
await wait(500);
await shot('07b-export-mp4');
await page.locator('.dialog button', { hasText: 'Start export' }).click();
const t0 = Date.now();
while (Date.now() - t0 < 120000 && !(await page.evaluate(() => window.__exportCalls.some(c => c.cmd === 'video_finish')))) await wait(500);
const calls = await page.evaluate(() => window.__exportCalls);
const begin = calls.find(c => c.cmd === 'video_begin');
const frames = calls.filter(c => c.cmd === 'video_frame');
const ok = begin && begin.args.path === 'C:/renders/tidefire.mp4' && begin.args.settings.width === 320
  && 'H264' in begin.args.settings.codec && frames.length === 3
  && frames.every(f => f.bytes === 320 * 180 * 4 && f.id === '7') && calls.some(c => c.cmd === 'video_finish');
log.push(`video export check: ${ok ? 'PASS' : 'FAIL'} ${JSON.stringify(calls.map(c => c.cmd))} ${JSON.stringify(begin?.args)}`);
fs.writeFileSync(`${out}/video-export-check.txt`, ok ? 'PASS' : 'FAIL');
await shot('07c-export-done');
await page.evaluate(() => { delete window.__TAURI__; });
await page.keyboard.press('Escape');

// Appearance: the dialog, then the Tide theme applied to the whole interface.
await page.locator('button', { hasText: 'Appearance' }).click();
await shot('08-appearance');
await page.locator('.dialog .seg', { hasText: 'Tide' }).click();
await shot('09-appearance-tide');
await page.locator('.dialog button', { hasText: 'Done' }).click();
await shot('10-tide-theme');
} catch (e) {
  log.push(`!!! script: ${e.message.split('\n')[0]}`);
} finally {
  fs.writeFileSync(`${out}/console.txt`, log.join('\n'));
  console.log(log.join('\n'));
  await browser.close();
}
