// Helpers compartilhados do e2e (uso manual, fora do CI):
//   CHROME_PATH=/usr/bin/google-chrome-stable  (ou autodetecta)
//   NSEC=nsec1...  (ou gera throwaway automaticamente — NUNCA commite nsec)
const { execSync } = require('child_process');

function chromePath() {
  if (process.env.CHROME_PATH) return process.env.CHROME_PATH;
  for (const c of ['google-chrome-stable', 'chromium', 'chromium-browser']) {
    try {
      const p = execSync(`which ${c}`, { stdio: ['ignore', 'pipe', 'ignore'] }).toString().trim();
      if (p) return p;
    } catch {}
  }
  throw new Error('Chrome não achado (defina CHROME_PATH)');
}

function launchOpts() {
  return {
    executablePath: chromePath(),
    args: ['--no-sandbox', '--disable-dev-shm-usage', '--disable-gpu', '--renderer-process-limit=1', '--disable-extensions', '--mute-audio'],
  };
}

function ensureNsec() {
  if (process.env.NSEC) return process.env.NSEC;
  const { generateSecretKey } = require('nostr-tools/pure');
  const { nsecEncode } = require('nostr-tools/nip19');
  const nsec = nsecEncode(generateSecretKey());
  console.log('NSEC gerado (throwaway, não commitar)');
  return nsec;
}

module.exports = { chromePath, launchOpts, ensureNsec };
