'use strict';
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const { createRequire } = require('node:module');
const { openTemporaryConsoleSession, rebaseStorageStateCookies } = require('../page-debug/auth');
const route = '/api/console/managed-services/langfuse/';
function connectionUrl(value) {
  const url = new URL(value);
  if (!url.pathname.endsWith('/')) url.pathname += '/';
  return url.href;
}
async function capture({ args, credentials, langfuse, sources, target, receipt, waitRemote, verifyTarget }) {
  const playwright = createRequire(path.join(args.credentialsRoot,'web/package.json'))('playwright');
  const storage = path.join(args.outDir,'private/console-storage.json');
  let owner, browser;
  const traceIds = new Set();
  try {
    owner = await openTemporaryConsoleSession({playwright,apiBaseUrl:args.apiBaseUrl,...credentials,storageStatePath:storage});
    fs.chmodSync(storage,0o600); rebaseStorageStateCookies(storage,args.webBaseUrl);
    for (const [name,viewport] of [['desktop',{width:1440,height:1000}],['mobile',{width:390,height:844}]]) {
      browser = await playwright.chromium.launch({executablePath:'/usr/bin/google-chrome',headless:true});
      const context = await browser.newContext({viewport,storageState:storage});
      try {
        const page = await context.newPage(); page.setDefaultTimeout(30000);
        await page.goto(args.webBaseUrl+'/settings/langfuse?language=en',{waitUntil:'domcontentloaded'});
        const settings = page.getByTestId('langfuse-settings'); await settings.waitFor();
        if (await settings.getByRole('button',{name:'English',exact:true}).isVisible()) await settings.getByRole('button',{name:'English',exact:true}).click();
        const forms = settings.locator('form');
        const config = forms.nth(0), exporter = forms.nth(1), importer = forms.nth(2);
        const chooseApplication = async (form, label) => {
          const combo = form.getByRole('combobox'); await combo.click();
          const match = (await combo.getAttribute('class')).match(/([^\s]+)-select-input/);
          assert.ok(match, 'managed Select exposes its isolated component prefix');
          const popup = page.locator('.' + match[1] + '-select-dropdown:visible'); await popup.waitFor();
          const listId = await combo.getAttribute('aria-controls'); assert.ok(listId, 'expanded Select owns its list');
          const holder = popup.locator('.' + match[1] + '-select-dropdown-list-holder');
          const deadline = Date.now() + args.deadlineMs;
          for (;;) {
            const option = popup.getByText(label, { exact: true });
            if (await option.count()) { await option.click(); return; }
            const before = await holder.evaluate(el => ({ top: el.scrollTop, height: el.clientHeight, total: el.scrollHeight }));
            assert.ok(before.top + before.height < before.total, 'application option absent from complete dropdown: ' + label);
            assert.ok(Date.now() < deadline, 'application dropdown scrolling deadline');
            await holder.hover(); await page.mouse.wheel(0, before.height);
            await page.waitForFunction(({listId, prefix, top}) => {
              const holder = document.getElementById(listId)?.closest('.' + prefix + '-select-dropdown')?.querySelector('.' + prefix + '-select-dropdown-list-holder');
              return holder && holder.scrollTop !== top;
            }, {listId, prefix:match[1], top:before.top},
              {timeout:Math.max(1, deadline - Date.now())});
          }
        };
        const settledReceipt = async value => {
          const box = settings.getByTestId('langfuse-receipt'); await box.waitFor();
          const labels = { exported_records:'Exported records', exported_spans:'Exported spans', imported_traces:'Imported traces', accepted_events:'Accepted events', duplicate_events:'Duplicate events' };
          for (const [key, label] of Object.entries(labels)) if (value[key] !== undefined) {
            await box.getByText(`${label}: ${value[key]}`, { exact:true }).waitFor();
          }
        };
        const screenshot = async label => {
          const output = path.join(args.outDir,`${name}-${label}.png`);
          await page.screenshot({path:output,fullPage:true,animations:'disabled',mask:[config.getByLabel('Public key',{exact:true}),config.getByLabel('Secret key',{exact:true})]});
          receipt.screenshots.push({path:output,viewport});
        };
        const action = async (operation,button,method='POST') => {
          const pending = page.waitForResponse(r => new URL(r.url()).pathname === route+operation && r.request().method() === method,{timeout:args.deadlineMs});
          await button.click(); const response = await pending;
          assert.ok(response.ok(), `${operation} HTTP ${response.status()}`);
          const envelope = await response.json(); const result = envelope.data;
          assert.equal(result?.success,true,`${operation}: ${result?.error?.code || 'missing success'}`);
          return result;
        };
        await config.getByLabel('Service URL',{exact:true}).fill(langfuse.base_url);
        await config.getByLabel('Public key',{exact:true}).fill(langfuse.public_key);
        await config.getByLabel('Secret key',{exact:true}).fill(langfuse.secret_key);
        const saved = await action('config',config.getByRole('button',{name:'Save',exact:true}),'PUT');
        assert.equal(saved.credential_configured,true);
        assert.equal(saved.base_url,connectionUrl(langfuse.base_url));
        assert.ok(!Object.hasOwn(saved,'public_key') && !Object.hasOwn(saved,'secret_key'));
        await action('applications',config.getByRole('button',{name:'Reload',exact:true}),'GET');
        assert.equal(await config.getByLabel('Service URL',{exact:true}).inputValue(),saved.base_url);
        assert.equal(await config.getByLabel('Secret key',{exact:true}).inputValue(),'');
        const connected = await action('test',config.getByRole('button',{name:'Test connection',exact:true}));
        assert.equal(connected.connected,true);
        await screenshot('01-saved-connection');
        receipt.checks.push({name:`${name} browser config save/reload/real connection`,status:'pass'});
        for (const source of sources) {
          await chooseApplication(exporter, `${source.name} (${source.application_type})`);
          await exporter.getByLabel('Started at or after (RFC3339)',{exact:true}).fill(source.started_at_from);
          await exporter.getByLabel('Started before (RFC3339)',{exact:true}).fill(source.started_at_to);
          const result = await action('export',exporter.getByRole('button',{name:'Export complete records',exact:true}));
          assert.ok(result.receipt.exported_records > 0 && result.receipt.exported_spans > 0);
          if (source.expected_events) assert.equal(result.receipt.exported_spans,source.expected_events);
          for (const id of result.receipt.trace_ids) traceIds.add(id);
          await settledReceipt(result.receipt);
          await screenshot(`02-export-${source.application_type}`);
          receipt.exports.push({viewport:name,application_id:source.application_id,...result.receipt});
        }
        const remote = await waitRemote([...traceIds], receipt.exports.filter(item => item.viewport === name).reduce((sum, item) => sum + item.exported_spans, 0));
        await chooseApplication(importer, target.name);
        await importer.getByLabel('Trace IDs (one per line or comma-separated)',{exact:true}).fill([...traceIds].join('\n'));
        const imported = await action('import',importer.getByRole('button',{name:'Import complete traces',exact:true}));
        assert.ok(imported.receipt.accepted_events + imported.receipt.duplicate_events > 0);
        receipt.imports.push({viewport:name,...imported.receipt});
        await settledReceipt(imported.receipt);
        await screenshot('04-import-receipt');
        const repeat = await action('import',importer.getByRole('button',{name:'Import complete traces',exact:true}));
        assert.equal(repeat.receipt.accepted_events,0);
        assert.equal(repeat.receipt.duplicate_events,imported.receipt.accepted_events+imported.receipt.duplicate_events);
        assert.deepEqual([...repeat.receipt.record_ids].sort(),[...imported.receipt.record_ids].sort());
        await settledReceipt(repeat.receipt);
        receipt.checks.push({name:`${name} browser complete import and dedup`,status:'pass',...repeat.receipt});
        const synthetic = sources.find(source => source.application_type === 'agent_logs');
        const syntheticExport = receipt.exports.find(item => item.viewport === name && item.application_id === synthetic.application_id);
        await importer.getByRole('switch', { name:'Select traces by date', exact:true }).click();
        await importer.getByLabel('Started at or after (RFC3339)', { exact:true }).fill(synthetic.started_at_from);
        await importer.getByLabel('Started before (RFC3339)', { exact:true }).fill(synthetic.started_at_to);
        const dateImport = await action('import', importer.getByRole('button', { name:'Import complete traces', exact:true }));
        assert.equal(dateImport.receipt.accepted_events, 0);
        assert.equal(dateImport.receipt.duplicate_events, synthetic.expected_events);
        assert.equal(dateImport.receipt.imported_traces, syntheticExport.trace_ids.length);
        await settledReceipt(dateImport.receipt);
        await screenshot('04-date-import-receipt');
        receipt.checks.push({ name:`${name} browser date import complete traces`, status:'pass', ...dateImport.receipt });
        const fieldIds = await settings.locator('input[id],textarea[id]').evaluateAll(inputs => inputs.map(input => input.id));
        assert.equal(new Set(fieldIds).size, fieldIds.length, 'unique form field IDs');
        receipt.checks.push({ name:`${name} unique form IDs and source/target/date label associations`, status:'pass' });
        // Full browser reload proves persisted connection, rather than component state alone.
        await page.reload({waitUntil:'domcontentloaded'});
        await config.locator('input[placeholder="https://cloud.langfuse.com/"]').waitFor();
        await page.waitForFunction(url => document.querySelector('input[placeholder="https://cloud.langfuse.com/"]')?.value === url,saved.base_url);
        const importedRecordId = await verifyTarget(imported.receipt.record_ids);
        await page.goto(`${args.webBaseUrl}/applications/${target.application_id}/logs?language=en&run_id=${encodeURIComponent(importedRecordId)}`,{waitUntil:'domcontentloaded'});
        await page.getByTestId('application-logs-floating-run-detail').getByText('Synthetic answer 2.',{exact:true}).waitFor();
        await screenshot('05-imported-logs');
        if (args.langfuseStoragePrivateFile) {
          const external = await browser.newContext({viewport,storageState:args.langfuseStoragePrivateFile});
          try {
            const remotePage = await external.newPage();
            const publicationTraceId = syntheticExport.trace_ids[0];
            const url = new URL(`project/${encodeURIComponent(langfuse.project_id)}/traces/${encodeURIComponent(publicationTraceId)}`, connectionUrl(langfuse.base_url));
            url.searchParams.set('timestamp', remote[publicationTraceId].start_time);
            await remotePage.goto(url.href, {waitUntil:'domcontentloaded'});
            await remotePage.getByText(/^agent-log\./).first().waitFor({state:'visible',timeout:60000});
            const output=path.join(args.outDir,`${name}-03-langfuse-traces.png`);
            await remotePage.screenshot({path:output,fullPage:true,animations:'disabled'}); receipt.screenshots.push({path:output,viewport,external:true,trace_id:publicationTraceId,source_kind:'synthetic'});
          } finally { await external.close(); }
        }
      } finally { await context.close(); await browser.close(); browser=null; }
    }
    return [...traceIds];
  } finally { if(browser) await browser.close(); try {if(owner) await owner.dispose();} finally {fs.rmSync(storage,{force:true});} }
}
module.exports={capture};
