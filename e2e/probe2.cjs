const { chromium } = require('playwright-core');

(async () => {
  const browser = await chromium.launch({
    executablePath: '/usr/bin/google-chrome-stable',
    args: ['--no-sandbox', '--disable-dev-shm-usage'],
  });
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  await page.goto('https://armada.buzz', { waitUntil: 'domcontentloaded', timeout: 45000 });
  await page.waitForTimeout(6000);
  await page.getByRole('button', { name: 'Join' }).first().click();
  await page.waitForTimeout(8000);
  console.log('URL:', page.url());
  const els = await page.$$eval('button, a, input, textarea, [role="button"], [role="textbox"]', (nodes) =>
    nodes.slice(0, 80).map((n) => ({
      tag: n.tagName,
      type: n.getAttribute('type') || '',
      text: (n.innerText || n.value || n.getAttribute('aria-label') || n.getAttribute('placeholder') || '').slice(0, 100),
    }))
  );
  console.log(JSON.stringify(els, null, 1));
  await page.screenshot({ path: 'probe2.png' });
  await browser.close();
})().catch((e) => { console.error('FATAL', e.message); process.exit(1); });
