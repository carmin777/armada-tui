// Uso: NSEC=... COMMURL=... node finish.cjs (NSEC ausente = gera throwaway)
const { chromium } = require('playwright-core');
const { launchOpts, ensureNsec } = require('./lib.cjs');
const NSEC = ensureNsec();
const COMMURL = process.env.COMMURL;
const HELLO = `ola da tui e2e ${Date.now()}`;

(async () => {
  const browser = await chromium.launch(launchOpts());
  const page = await browser.newPage({ viewport: { width: 1000, height: 750 } });
  await page.route('**/*.{png,jpg,jpeg,gif,webp,svg,woff,woff2,ttf,mp4,webm}', (r) => r.abort()).catch(() => {});
  page.on('console', (m) => { if (m.type() === 'error') console.log('[browser.err]', m.text().slice(0, 140)); });
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
  for (let i = 0; i < 10; i++) {
    await page.waitForTimeout(15000);
    const has = await page.locator('textarea[aria-label^="Message #"]').count();
    console.log(`t+${(i + 1) * 15}s boxes=${has}`);
    if (has > 0) break;
  }
  await page.screenshot({ path: 'before-msg.png' });
  await page.locator('textarea[aria-label^="Message #"]').fill(HELLO, { timeout: 45000 });
  await page.keyboard.press('Enter');
  await page.waitForTimeout(10000);
  console.log('HELLO:', HELLO);
  await page.screenshot({ path: 'after-msg.png' });
  // Fecha painel members se aberto, abre menu do servidor
  await page.getByRole('button', { name: 'Close members' }).click({ timeout: 10000 }).catch(() => {});
  await page.getByRole('button', { name: /tui-e2e-frota/ }).first().click({ timeout: 15000 });
  await page.waitForTimeout(3000);
  const menu = await page.$$eval('button, [role="menuitem"]', (nodes) =>
    nodes.slice(0, 40).map((n) => n.innerText.slice(0, 60))
  );
  console.log('SERVER MENU:', JSON.stringify(menu));
  await page.screenshot({ path: 'server-menu.png' });
  await browser.close();
})().catch((e) => { console.error('FATAL', e.message); process.exit(1); });
