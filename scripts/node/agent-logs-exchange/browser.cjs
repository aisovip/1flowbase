'use strict';
const fs = require('node:fs/promises');
const path = require('node:path');
const assert = require('node:assert/strict');
const { createRequire } = require('node:module');
const { openTemporaryConsoleSession, rebaseStorageStateCookies } = require('../page-debug/auth');

async function capture({ args, credentials, applicationId, recordIds, langfuse, receipt }) {
  const playwright = createRequire(path.join(args.credentialsRoot, 'web/package.json'))('playwright');
  const storageStatePath = path.join(args.outDir, 'private', 'console-storage.json');
  let owner, browser;
  try {
    owner = await openTemporaryConsoleSession({ playwright, apiBaseUrl: args.apiBaseUrl, ...credentials, storageStatePath });
    await fs.chmod(storageStatePath, 0o600);
    rebaseStorageStateCookies(storageStatePath, args.webBaseUrl);
    browser = await playwright.chromium.launch({ executablePath: '/usr/bin/google-chrome', headless: true });
    for (const [name, viewport] of [['desktop', { width: 1440, height: 1000 }], ['mobile', { width: 390, height: 844 }]]) {
      const context = await browser.newContext({ viewport, storageState: storageStatePath });
      try {
        const page = await context.newPage();
        page.setDefaultTimeout(30000);
        await page.goto(`${args.webBaseUrl}/applications/${applicationId}/logs?language=zh`, { waitUntil: 'domcontentloaded' });
        for (const id of recordIds.slice(0, 2)) await page.locator(`input[type=checkbox][name="${id}"]`).check();
        const exchange = page.getByRole('button', { name: /^(日志交换|Exchange logs)$/ });
        if (!(await exchange.isVisible())) await page.getByRole('button', { name: /更多筛选和操作|More filters and actions/ }).click();
        await exchange.click();
        const dialog = page.getByRole('dialog');
        await dialog.waitFor({ state: 'visible' });
        assert.ok(await dialog.getByText(/已选择 2 条日志|2 logs selected/).isVisible());
        const save = async (suffix, mask = []) => {
          const output = path.join(args.outDir, `${name}-${suffix}.png`);
          await page.screenshot({ path: output, fullPage: true, animations: 'disabled', mask });
          receipt.screenshots.push({ path: output, viewport, application_id: applicationId });
        };
        await save('otlp-exchange');
        await dialog.getByRole('combobox', { name: /交换方式|Exchange operation/ }).click();
        await page.getByText(/^(导出到 Langfuse|Export to Langfuse)$/, { exact: true }).click();
        await dialog.getByLabel(/Langfuse 地址|Langfuse URL/).fill(langfuse.base_url);
        await dialog.getByLabel('Public Key', { exact: true }).fill(langfuse.public_key);
        await dialog.getByLabel('Secret Key', { exact: true }).fill(langfuse.secret_key);
        await save('langfuse-exchange', [dialog.getByLabel('Public Key', { exact: true }), dialog.getByLabel('Secret Key', { exact: true })]);
        await dialog.getByRole('button', { name: /^(关闭|Close)$/ }).click();
        receipt.checks.push({ name: `${name} exchange controls and masked credentials`, status: 'pass' });
      } finally { await context.close(); }
    }
    if (args.langfuseStoragePrivateFile) {
      const context = await browser.newContext({ storageState: args.langfuseStoragePrivateFile, viewport: { width: 1440, height: 1000 } });
      try {
        const page = await context.newPage();
        await page.goto(`${langfuse.base_url}/project/${encodeURIComponent(langfuse.project_id)}/traces`, { waitUntil: 'domcontentloaded' });
        await page.getByRole('main').waitFor({ state: 'visible', timeout: 30000 });
        const output = path.join(args.outDir, 'langfuse-traces.png');
        await page.screenshot({ path: output, fullPage: true, animations: 'disabled' });
        receipt.screenshots.push({ path: output, scope: 'Langfuse traces page only; API readiness is checked separately' });
      } finally { await context.close(); }
    }
  } finally {
    try { if (browser) await browser.close(); }
    finally {
      try { if (owner) await owner.dispose(); }
      finally { await fs.rm(storageStatePath, { force: true }); }
    }
  }
}
module.exports = { capture };
