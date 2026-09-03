// Uso: NSEC=... node mkserver.cjs
const { chromium } = require('playwright-core');
const NSEC = process.env.NSEC;

(async () => {
  const browser = await chromium.launch({
    executablePath: '/usr/bin/google-chrome-stable',
    args: ['--no-sandbox', '--disable-dev-shm-usage', '--disable-gpu', '--renderer-process-limit=1', '--disable-extensions', '--mute-audio'],
  });
  const page = await browser.newPage({ viewport: { width: 1000, height: 750 } });
  // economiza RAM: bloqueia imagens/fontes/mídia (forms continuam ok)
  await page.route('**/*.{png,jpg,jpeg,gif,webp,svg,woff,woff2,ttf,mp4,webm}', (r) => r.abort()).catch(() => {});
  await page.goto('https://armada.buzz', { waitUntil: 'domcontentloaded', timeout: 45000 });
  await page.waitForTimeout(6000);
  await page.getByRole('button', { name: 'Join' }).first().click();
  await page.waitForTimeout(3000);
  await page.getByPlaceholder('nsec1… or bunker://…').fill(NSEC);
  await page.getByRole('button', { name: 'Log in' }).click();
  await page.waitForTimeout(10000);
  console.log('LOGGED URL:', page.url());
  // espera o splash de loading sair
  await page.locator('div[role="status"]').first().waitFor({ state: 'detached', timeout: 40000 }).catch(() => {});
  await page.waitForTimeout(3000);
  await page.screenshot({ path: 'after-splash.png' });
  // tela "restore your setup" → Skip for now (robusto)
  await page.getByText('Skip for now').waitFor({ timeout: 25000 });
  await page.getByText('Skip for now').click({ timeout: 15000 });
  await page.waitForFunction(
    () => ![...document.querySelectorAll('div')].some((d) => d.textContent.includes('restore your setup')),
    { timeout: 20000 }
  ).catch(() => {});
  await page.waitForTimeout(4000);
  await page.screenshot({ path: 'after-skip.png' });
  await page.getByRole('button', { name: 'Add a server or encrypted chat' }).click({ timeout: 15000 });
  await page.waitForTimeout(4000);
  await page.getByRole('button', { name: 'Create encrypted community' }).click();
  await page.waitForTimeout(6000);
  await page.screenshot({ path: 'after-click-create.png' });
  const dlg2 = await page.$$eval('h1, h2, h3, button, input', (nodes) =>
    nodes.slice(0, 40).map((n) => `${n.tagName}: ${(n.innerText || n.getAttribute('placeholder') || '').slice(0, 80)}`)
  );
  console.log('AFTER CREATE CLICK:', JSON.stringify(dlg2));
  await page.getByPlaceholder('e.g. Midnight Fleet').fill('tui-e2e-frota', { timeout: 30000 });
  await page.getByRole('button', { name: 'Continue' }).click();
  await page.waitForTimeout(6000);
  await page.screenshot({ path: 'after-face.png' });
  console.log('URL:', page.url());
  // "give it a face" → Continue de novo (sem banner/ícone)
  await page.getByRole('button', { name: 'Continue' }).click({ timeout: 15000 }).catch(() => {});
  await page.waitForTimeout(8000);
  await page.screenshot({ path: 'after-face2.png' });
  console.log('URL2:', page.url());
  // "where it lives" → mantém só ditto+dreamith (menos publishes, mesmo teste)
  await page.getByRole('button', { name: 'Remove wss://jskitty.com/nostr' }).click({ timeout: 15000 }).catch(() => {});
  await page.getByRole('button', { name: 'Remove wss://asia.vectorapp.io/nostr' }).click({ timeout: 15000 }).catch(() => {});
  await page.screenshot({ path: 'relays-trimmed.png' });
  await page.getByRole('button', { name: 'Continue' }).click({ timeout: 15000 });
  await page.waitForTimeout(10000);
  await page.screenshot({ path: 'after-relays.png' });
  console.log('URL3:', page.url());
  // "disappearing messages" → Create encrypted community (final)
  page.on('console', (m) => { if (m.type() === 'error' || m.type() === 'warning') console.log('[browser]', m.type(), m.text().slice(0, 160)); });
  await page.getByRole('button', { name: 'Create encrypted community' }).click({ timeout: 15000 });
  for (let i = 0; i < 20; i++) {
    await page.waitForTimeout(15000);
    const url = page.url();
    const btn = await page.getByRole('button', { name: /Creat/ }).first().innerText().catch(() => '(sem botão)');
    console.log(`t+${(i + 1) * 15}s url=${url} btn=${btn.slice(0, 30)}`);
    if (!url.includes('/create')) break;
  }
  await page.waitForTimeout(5000);
  // DENTRO DA COMUNIDADE: manda mensagem no #general
  const helloMsg = `ola da tui e2e ${Date.now()}`;
  await page.getByPlaceholder(/Message #/).fill(helloMsg, { timeout: 120000 });
  await page.keyboard.press('Enter');
  await page.waitForTimeout(8000);
  await page.screenshot({ path: 'after-msg.png' });
  // Abre menu do servidor → Invite
  await page.getByRole('button', { name: /tui-e2e-frota/ }).first().click({ timeout: 15000 }).catch(() => {});
  await page.waitForTimeout(3000);
  const menu = await page.$$eval('button, [role="menuitem"]', (nodes) =>
    nodes.slice(0, 40).map((n) => n.innerText.slice(0, 60))
  );
  console.log('SERVER MENU:', JSON.stringify(menu));
  console.log('HELLO:', helloMsg);
  const els = await page.$$eval('button, a, input, textarea, h1, h2, h3', (nodes) =>
    nodes.slice(0, 80).map((n) => ({
      tag: n.tagName,
      text: (n.innerText || n.value || n.getAttribute('aria-label') || n.getAttribute('placeholder') || '').slice(0, 90),
    }))
  );
  console.log(JSON.stringify(els, null, 1));
  await page.screenshot({ path: 'mkserver.png' });
  await browser.close();
})().catch((e) => { console.error('FATAL', e.message); process.exit(1); });
