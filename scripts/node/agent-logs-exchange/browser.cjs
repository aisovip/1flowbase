'use strict';
const fs = require('node:fs/promises');
const path = require('node:path');
const assert = require('node:assert/strict');
const { createRequire } = require('node:module');
const { openTemporaryConsoleSession, rebaseStorageStateCookies } = require('../page-debug/auth');

async function capture({ args, credentials, applicationId, recordIds, targetApplicationId, importedRecordIds, conversation, traceIds, eventCount, langfuse, receipt }) {
  assert.ok(args.langfuseStoragePrivateFile, 'Langfuse private storage required for five Wiki images');
  const playwright = createRequire(path.join(args.credentialsRoot, 'web/package.json'))('playwright');
  const storageStatePath = path.join(args.outDir, 'private', 'console-storage.json');
  const viewports = [['desktop', { width: 1440, height: 1000 }], ['mobile', { width: 390, height: 844 }]];
  let owner, browser;
  try {
    owner = await openTemporaryConsoleSession({ playwright, apiBaseUrl: args.apiBaseUrl, ...credentials, storageStatePath });
    await fs.chmod(storageStatePath, 0o600);
    rebaseStorageStateCookies(storageStatePath, args.webBaseUrl);
    for (const [name, viewport] of viewports) {
      // Release renderer/navigation caches between viewport evidence batches.
      browser = await playwright.chromium.launch({ executablePath: '/usr/bin/google-chrome', headless: true });
      const context = await browser.newContext({ viewport, storageState: storageStatePath });
      try {
        const page = await context.newPage();
        page.setDefaultTimeout(30000);
        const save = async (filename, app, mask = []) => {
          const output = path.join(args.outDir, name === 'desktop' ? filename : `${name}-${filename}`);
          await page.screenshot({ path: output, fullPage: true, animations: 'disabled', mask });
          receipt.screenshots.push({ path: output, viewport, application_id: app });
        };
        const openExchange = async () => {
          const exchange = page.getByRole('button', { name: /^(日志交换|Exchange logs)$/ });
          if (!(await exchange.isVisible())) await page.getByRole('button', { name: /更多筛选和操作|More filters and actions/ }).click();
          await exchange.click();
          const dialog = page.getByRole('dialog');
          await dialog.waitFor({ state: 'visible' });
          return dialog;
        };
        const chooseOperation = async (dialog, operation) => {
          await dialog.getByRole('combobox', { name: /交换方式|Exchange operation/ }).click();
          await page.locator('.ant-select-item-option-content').filter({ hasText: operation }).click();
        };
        const fillCredentials = async (dialog) => {
          await dialog.getByLabel(/Langfuse 地址|Langfuse URL/).fill(langfuse.base_url);
          await dialog.getByLabel('Public Key', { exact: true }).fill(langfuse.public_key);
          await dialog.getByLabel('Secret Key', { exact: true }).fill(langfuse.secret_key);
          return [dialog.getByLabel('Public Key', { exact: true }), dialog.getByLabel('Secret Key', { exact: true })];
        };
        await page.goto(`${args.webBaseUrl}/applications/${applicationId}/logs?language=zh`, { waitUntil: 'domcontentloaded' });
        // Canonical rows expose only the latest task in each conversation.
        const sourceSelector = recordIds.map(id => `input[type=checkbox][name="${id}"]`).join(', ');
        const selected = new Set();
        if (name === 'mobile') {
          // Virtualized rows mount as the user scrolls; select each source identity once.
          const list = page.locator('.data-table-mobile__viewport .ant-listy-holder');
          await list.waitFor({ state: 'visible' });
          const dimensions = await list.evaluate(element => ({ height: element.clientHeight, total: element.scrollHeight }));
          const maxSteps = Math.ceil(dimensions.total / (dimensions.height * 0.7)) + 2;
          for (let step = 0; step < maxSteps; step++) {
            for (const id of recordIds) {
              const row = page.locator(`input[type=checkbox][name="${id}"]`);
              if (!selected.has(id) && await row.count()) {
                await row.check();
                selected.add(id);
              }
            }
            const metrics = await list.evaluate(element => ({ top: element.scrollTop, height: element.clientHeight, total: element.scrollHeight }));
            if (metrics.top + metrics.height >= metrics.total - 1) break;
            await list.hover();
            await page.mouse.wheel(0, metrics.height * 0.7);
            await page.waitForFunction(({ selector, previous }) => document.querySelector(selector).scrollTop > previous,
              { selector: '.data-table-mobile__viewport .ant-listy-holder', previous: metrics.top });
          }
        } else {
          const sourceRows = page.locator(sourceSelector);
          await sourceRows.nth(1).waitFor({ state: 'visible' });
          assert.equal(await sourceRows.count(), 2, 'three task facts render two canonical conversations');
          for (const row of await sourceRows.all()) {
            selected.add(await row.getAttribute('name'));
            await row.check();
          }
        }
        assert.equal(selected.size, 2, 'both viewports select exactly two canonical source conversations');
        await save('01-record-selection.png', applicationId);
        const dialog = await openExchange();
        assert.ok(await dialog.getByText(/已选择 2 条日志|2 logs selected/).isVisible());
        await save('otlp-exchange.png', applicationId);
        await chooseOperation(dialog, /^(导出到 Langfuse|Export to Langfuse)$/);
        const exportMasks = await fillCredentials(dialog);
        await save('02-langfuse-export.png', applicationId, exportMasks);
        const exportRoute = `/api/console/applications/${applicationId}/logs/langfuse/export`;
        const exportResponse = page.waitForResponse(response => new URL(response.url()).pathname === exportRoute && response.request().method() === 'POST', { timeout: args.deadlineMs });
        await dialog.getByRole('button', { name: /^(执行交换|Execute exchange)$/ }).click();
        const exportedResponse = await exportResponse;
        assert.ok(exportedResponse.ok(), 'UI Langfuse export must receive HTTP success');
        const exportBody = exportedResponse.request().postDataJSON();
        assert.equal(exportBody.log_conversation_ids.length, 2, 'UI selects two canonical conversations');
        assert.equal(new Set(exportBody.log_conversation_ids).size, 2);
        assert.ok(!Object.hasOwn(exportBody, 'record_ids'), 'UI must not export only latest tasks');
        const exported = (await exportedResponse.json()).data;
        assert.equal(exported.exported_records, 3, 'two conversations export all three task facts');
        assert.equal(exported.exported_spans, eventCount, 'UI exports all raw spans');
        assert.equal(exported.trace_ids.length, 2);
        assert.deepEqual([...exported.trace_ids].sort(), [...traceIds].sort(), 'UI retains the API-verified trace identities');
        await dialog.getByText(/^(服务端交换回执|Server exchange receipt)$/).waitFor({ state: 'visible' });
        assert.equal(await dialog.getByText(/仅导出了部分|exported only part/).count(), 0, 'conversation expansion is not a partial failure');
        receipt.checks.push({ name: `${name} real UI Langfuse complete conversation export`, status: 'pass', application_id: applicationId, log_conversation_ids: exportBody.log_conversation_ids, exported_records: exported.exported_records, exported_spans: exported.exported_spans, trace_ids: exported.trace_ids });
        await dialog.locator('.ant-modal-footer').getByRole('button', { name: /^(关\s*闭|Close)$/ }).click();

        await page.goto(`${args.webBaseUrl}/applications/${targetApplicationId}/logs?language=zh`, { waitUntil: 'domcontentloaded' });
        const targetRows = page.locator(importedRecordIds.map(id => `input[type=checkbox][name="${id}"]`).join(', '));
        await targetRows.first().waitFor({ state: 'visible' });
        const importDialog = await openExchange();
        await chooseOperation(importDialog, /^(从 Langfuse 导入|Import from Langfuse)$/);
        const importMasks = await fillCredentials(importDialog);
        await importDialog.getByLabel(/Trace IDs/).fill(traceIds.join('\n'));
        await save('04-langfuse-import.png', targetApplicationId, importMasks);
        // Repeat the already API-validated import through the actual UI on both viewports.
        const route = `/api/console/applications/${targetApplicationId}/logs/langfuse/import`;
        const exchangeResponse = page.waitForResponse(response => new URL(response.url()).pathname === route && response.request().method() === 'POST', { timeout: args.deadlineMs });
        await importDialog.getByRole('button', { name: /^(执行交换|Execute exchange)$/ }).click();
        const response = await exchangeResponse;
        assert.ok(response.ok(), 'UI Langfuse import must receive HTTP success');
        assert.deepEqual(response.request().postDataJSON().trace_ids, traceIds, 'UI sends the returned trace IDs');
        const imported = (await response.json()).data;
        assert.equal(imported.accepted_events, 0, 'UI repeated import accepts zero new events');
        assert.equal(imported.duplicate_events, eventCount, 'UI repeated import reports all duplicates');
        assert.deepEqual([...imported.record_ids].sort(), [...importedRecordIds].sort(), 'UI repeat retains record identities');
        await importDialog.getByText(/^(服务端交换回执|Server exchange receipt)$/).waitFor({ state: 'visible' });
        assert.ok(await importDialog.getByText('0', { exact: true }).isVisible(), 'UI renders accepted count');
        assert.ok(await importDialog.getByText(String(eventCount), { exact: true }).isVisible(), 'UI renders duplicate count');
        await save('langfuse-import-receipt.png', targetApplicationId, importMasks);
        receipt.checks.push({ name: `${name} real UI Langfuse repeat import`, status: 'pass', application_id: targetApplicationId, accepted_events: imported.accepted_events, duplicate_events: imported.duplicate_events, record_ids: imported.record_ids, trace_ids: traceIds });
        await importDialog.locator('.ant-modal-footer').getByRole('button', { name: /^(关\s*闭|Close)$/ }).click();
        await page.goto(`${args.webBaseUrl}/applications/${targetApplicationId}/logs?language=zh&run_id=${encodeURIComponent(conversation.record_id)}`, { waitUntil: 'domcontentloaded' });
        const details = page.getByTestId('application-logs-floating-run-detail');
        await details.getByText(conversation.user, { exact: true }).waitFor({ state: 'visible' });
        await details.getByText(conversation.final_answer, { exact: true }).waitFor({ state: 'visible' });
        await save('05-imported-conversation.png', targetApplicationId);
        receipt.checks.push({ name: `${name} imported source user and final conversation visible`, status: 'pass', application_id: targetApplicationId, record_id: conversation.record_id });
      } finally {
        try { await context.close(); }
        finally { await browser.close(); browser = null; }
      }
    }
    for (const [name, viewport] of viewports) {
      // Release renderer/navigation caches between viewport evidence batches.
      browser = await playwright.chromium.launch({ executablePath: '/usr/bin/google-chrome', headless: true });
      const context = await browser.newContext({ storageState: args.langfuseStoragePrivateFile, viewport });
      try {
        const page = await context.newPage();
        await page.goto(`${langfuse.base_url}/project/${encodeURIComponent(langfuse.project_id)}/traces`, { waitUntil: 'domcontentloaded' });
        await page.getByRole('main').waitFor({ state: 'visible', timeout: 30000 });
        await page.locator('tbody tr').first().waitFor({ state: 'visible', timeout: 30000 });
        const output = path.join(args.outDir, name === 'desktop' ? '03-langfuse-traces.png' : `${name}-03-langfuse-traces.png`);
        await page.screenshot({ path: output, fullPage: true, animations: 'disabled' });
        receipt.screenshots.push({ path: output, viewport, scope: 'Langfuse traces page only; API readiness is checked separately' });
      } finally {
        try { await context.close(); }
        finally { await browser.close(); browser = null; }
      }
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
