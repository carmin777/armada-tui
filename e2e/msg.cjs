// Uso: NSEC=... COMMURL=https://armada.buzz/c/... node msg.cjs
const { chromium } = require('playwright-core');
const NSEC = process.env.NSEC;
const COMMURL = process.env.COMMURL;

(async () => {
  const browser = await chromium.launch({
    executablePath: '/usr/bin/google-chrome-stable',
    args: ['--no-sandbox', '--disable-dev-shm-usage', '--disable-gpu', '--renderer-process-limit=1', '--disable-extensions', '--mute-audio'],
  });
  const page = await browser.newPage({ viewport: { width: 1000, height: 750 } });
  await page.route('**/*.{png,jpg,jpeg,gif,webp,svg,woff,woff2,ttf,mp4,webm}', (r) => r.abort()).catch(() => {});
  await page.goto('https://armada.buzz', { waitUntil: 'domcontentloaded', timeout: 45000 });
  await page.waitForTimeout(6000);
  await page.getByRole('button', { name: 'Join' }).first().click();
  await page.waitForTimeout(3000);
  await page.getByPlaceholder('nsec1… or bunker://…').fill(NSEC);
  await page.getByRole('button', { name: 'Log in' }).click();
  await page.waitForTimeout(8000);
  await page.getByText('Skip for now').click({ timeout: 20000 }).catch(() => {});
  await page.waitForTimeout(4000);
  await page.goto(COMMURL, { waitUntil: 'domcontentloaded', timeout: 45000 });
  await page.waitForTimeout(12000);
  console.log('URL:', page.url());
  const boxes = await page.$$eval('[contenteditable="true"], [role="textbox"], textarea, input:not([type="hidden"]):not([type="file"])', (nodes) =>
    nodes.map((n) => `${n.tagName} ph=${(n.getAttribute('placeholder') || n.getAttribute('aria-label') || '').slice(0, 60)} text=${(n.innerText || '').slice(0, 60)}`)
  );
  console.log('BOXES:', JSON.stringify(boxes, null, 1));
  await page.screenshot({ path: 'msg-probe.png' });
  await browser.close();
})().catch((e) => { console.error('FATAL', e.message); process.exit(1); });
