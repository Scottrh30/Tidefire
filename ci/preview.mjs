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
