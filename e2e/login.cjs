// Uso: NSEC=nsec1... node login.cjs
const { chromium } = require('playwright-core');
const NSEC = process.env.NSEC;
if (!NSEC) { console.error('NSEC vazio'); process.exit(1); }

(async () => {
  const browser = await chromium.launch({
    executablePath: '/usr/bin/google-chrome-stable',
    args: ['--no-sandbox', '--disable-dev-shm-usage'],
  });
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  await page.goto('https://armada.buzz', { waitUntil: 'domcontentloaded', timeout: 45000 });
  await page.waitForTimeout(6000);
  await page.getByRole('button', { name: 'Join' }).first().click();
  await page.waitForTimeout(3000);
  await page.getByPlaceholder('nsec1… or bunker://…').fill(NSEC);
  await page.screenshot({ path: 'login-filled.png' });
  // submit = botão "Log in" do modal
  await page.getByRole('button', { name: 'Log in' }).click();
  await page.waitForTimeout(10000);
  console.log('URL:', page.url());
  const els = await page.$$eval('button, a, input, textarea, [role="button"], [role="textbox"], h1, h2, h3', (nodes) =>
    nodes.slice(0, 100).map((n) => ({
      tag: n.tagName,
      text: (n.innerText || n.value || n.getAttribute('aria-label') || n.getAttribute('placeholder') || '').slice(0, 90),
    }))
  );
  console.log(JSON.stringify(els, null, 1));
  await page.screenshot({ path: 'logged.png' });
  await browser.close();
})().catch((e) => { console.error('FATAL', e.message); process.exit(1); });
