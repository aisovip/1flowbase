'use strict';
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const { createRequire } = require('node:module');
const { openTemporaryConsoleSession, rebaseStorageStateCookies } = require('../page-debug/auth');
const route = '/api/console/managed-services/langfuse/';
const { chooseApplication } = require('./choose-application.cjs');
const { noSecrets } = require('./openapi.cjs');
function connectionUrl(value) {
  const url = new URL(value);
  if (!url.pathname.endsWith('/')) url.pathname += '/';
  return url.href;
}
async function capture({ args, credentials, langfuse, sources, target, receipt, waitRemote, verifyTarget, checkpoint, request, verifyInstance }) {
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
        await page.goto(args.webBaseUrl+'/settings/langfuse',{waitUntil:'domcontentloaded'});
        const settings = page.getByTestId('langfuse-settings'); await settings.waitFor();
        const table = settings.getByTestId('langfuse-instances');
        const dialog = title => page.getByRole('dialog',{name:title,exact:true});
        const row = id => table.getByTestId(`langfuse-instance-actions-${id}`);
        const close = async title => { await dialog(title).getByRole('button',{name:'Cancel',exact:true}).click(); await dialog(title).waitFor({state:'hidden'}); };
        const setLanguage = async language => {
          await page.getByLabel(/^(Switch language|切换语言)$/,{exact:true}).hover();
          const [response]=await Promise.all([
            page.waitForResponse(r=>new URL(r.url()).pathname==='/api/console/me/meta' && r.request().method()==='PATCH'),
            page.getByRole('menuitem',{name:language==='en_US'?'English':/^(Chinese|简体中文)$/,exact:true}).click()
          ]);
          assert.ok(response.ok(),'host language preference PATCH');
          const value=await response.json(); assert.equal(value.data.meta.ui.locale.preferred_locale,language);
          await table.getByRole('button',{name:language==='en_US'?'Add instance':'添加实例',exact:true}).waitFor();
          assert.equal(await settings.getByRole('button',{name:/^(English|Chinese|简体中文|Switch language|切换语言)$/}).count(),0,'no plugin language toggle');
        };
        await setLanguage('en_US');
        const settledReceipt = async value => {
          const box = settings.getByTestId('langfuse-receipt'); await box.waitFor();
          const labels = { exported_records:'Exported records', exported_spans:'Exported spans', imported_traces:'Imported traces', accepted_events:'Accepted events', duplicate_events:'Duplicate events' };
          for (const [key, label] of Object.entries(labels)) if (value[key] !== undefined) {
            await box.getByText(`${label}: ${value[key]}`, { exact:true }).waitFor();
          }
        };
        const screenshot = async label => {
          const output = path.join(args.outDir,`${name}-${label}.png`);
          await page.screenshot({path:output,fullPage:true,animations:'disabled',mask:[page.getByLabel('Public key',{exact:true}),page.getByLabel('Secret key',{exact:true})]});
          receipt.screenshots.push({path:output,viewport}); checkpoint();
        };
        const action = async (operation,button,method='POST',success=true) => {
          const [response] = await Promise.all([
            page.waitForResponse(r => new URL(r.url()).pathname === route+operation && r.request().method() === method,{timeout:args.deadlineMs}),
            button.click()
          ]);
          assert.ok(response.ok(), `${operation} HTTP ${response.status()}`);
          const envelope = await response.json(); const result = envelope.data;
          if(operation==='instances' && method==='POST' && result?.instance?.instance_id) {
            receipt.created_instance_ids.push(result.instance.instance_id); checkpoint();
          }
          noSecrets(result,[langfuse.public_key,langfuse.secret_key]);
          assert.equal(result?.success,success,`${operation}: ${result?.error?.code || 'missing success'}`);
          if(operation.startsWith('instances/') && method==='PUT') {
            const body=response.request().postDataJSON();
            assert.ok(!Object.hasOwn(body,'public_key') && !Object.hasOwn(body,'secret_key'),'blank edit omits credential pair');
          }
          if(['export','import'].includes(operation)) assert.equal(response.request().postDataJSON().instance_id,first.instance_id,'exchange explicitly targets selected row');
          return result;
        };
        async function create(name_, publicKey, secretKey) {
          await table.getByRole('button',{name:'Add instance',exact:true}).click();
          const form=dialog('Instance').locator('form');
          await form.getByLabel('Name',{exact:true}).fill(name_);
          await form.getByLabel('Service URL',{exact:true}).fill(langfuse.base_url);
          await form.getByLabel('Public key',{exact:true}).fill(publicKey);
          await form.getByLabel('Secret key',{exact:true}).fill(secretKey);
          const result=await action('instances',form.getByRole('button',{name:'Save',exact:true}));
          assert.equal(result.instance.credential_configured,true);
          assert.equal(result.instance.base_url,connectionUrl(langfuse.base_url));
          assert.ok(!Object.hasOwn(result.instance,'public_key') && !Object.hasOwn(result.instance,'secret_key'));
          await dialog('Instance').waitFor({state:'hidden'}); await row(result.instance.instance_id).waitFor();
          return result.instance;
        }
        const first=await create(`Acceptance ${name} primary ${receipt.run_id}`,langfuse.public_key,langfuse.secret_key);
        const second=await create(`Acceptance ${name} isolated ${receipt.run_id}`,`pk-invalid-${receipt.run_id}`,`sk-invalid-${receipt.run_id}`);
        assert.notEqual(first.instance_id,second.instance_id);
        await page.reload({waitUntil:'domcontentloaded'}); await row(first.instance_id).waitFor(); await row(second.instance_id).waitFor();
        if(name==='desktop') await table.getByRole('table').waitFor();
        else { assert.ok(await table.getByRole('article').count()>=2,'shared mobile list rows'); await table.getByRole('status').waitFor(); }
        await screenshot('00-instances-table-en');
        await row(first.instance_id).getByRole('button',{name:'Edit',exact:true}).click();
        const config=dialog('Instance').locator('form');
        assert.equal(await config.getByLabel('Service URL',{exact:true}).inputValue(),first.base_url);
        assert.equal(await config.getByLabel('Public key',{exact:true}).inputValue(),'');
        assert.equal(await config.getByLabel('Secret key',{exact:true}).inputValue(),'');
        const renamed=first.name+' renamed';
        await config.getByLabel('Name',{exact:true}).fill(renamed);
        const edited=await action(`instances/${first.instance_id}`,config.getByRole('button',{name:'Save',exact:true}),'PUT');
        const submitted=(await request('GET',route+'instances')).items.find(item=>item.instance_id===first.instance_id);
        assert.equal(edited.instance.instance_id,first.instance_id); assert.equal(submitted.name,renamed);
        await dialog('Instance').waitFor({state:'hidden'});
        await page.reload({waitUntil:'domcontentloaded'}); await table.getByText(renamed,{exact:true}).waitFor();
        const connected=await action(`instances/${first.instance_id}/test`,row(first.instance_id).getByRole('button',{name:'Test connection',exact:true}));
        assert.equal(connected.connected,true);
        const rejected=await action(`instances/${second.instance_id}/test`,row(second.instance_id).getByRole('button',{name:'Test connection',exact:true}),'POST',false);
        assert.ok(rejected.error?.code,'isolated invalid credential failure');
        await action(`instances/${first.instance_id}/test`,row(first.instance_id).getByRole('button',{name:'Test connection',exact:true}));
        await screenshot('01-saved-connection');
        await setLanguage('zh_Hans'); await screenshot('00-instances-table-zh');
        await row(first.instance_id).getByRole('button',{name:/^编\s*辑$/,exact:true}).click();
        await dialog('实例').getByLabel('名称',{exact:true}).waitFor();
        await screenshot('00-instance-drawer-zh');
        await dialog('实例').getByRole('button',{name:/^取\s*消$/,exact:true}).click(); await dialog('实例').waitFor({state:'hidden'});
        await setLanguage('en_US');
        await row(second.instance_id).getByRole('button',{name:'Delete',exact:true}).click();
        await action(`instances/${second.instance_id}`,page.getByRole('tooltip').getByRole('button',{name:'Delete',exact:true}),'DELETE');
        await row(second.instance_id).waitFor({state:'hidden'});
        const remaining=await request('GET',route+'instances');
        assert.ok(!remaining.items.some(item=>item.instance_id===second.instance_id));
        assert.ok(remaining.items.some(item=>item.instance_id===first.instance_id));
        await action(`instances/${first.instance_id}/test`,row(first.instance_id).getByRole('button',{name:'Test connection',exact:true}));
        receipt.checks.push({name:`${name} independent instances create/edit/blank-key preserve/reload/isolation/delete and global bilingual shared table`,status:'pass'}); checkpoint();
        if(name==='desktop') await verifyInstance(first.instance_id);
        for (const source of sources) {
          await row(first.instance_id).getByRole('button',{name:'Export',exact:true}).click();
          const exporter=dialog('Export application logs').locator('form');
          await chooseApplication({page,form:exporter,label:'Source application',applicationId:source.application_id,deadlineMs:args.deadlineMs});
          await exporter.getByLabel('Started at or after (RFC3339)',{exact:true}).fill(source.started_at_from);
          await exporter.getByLabel('Started before (RFC3339)',{exact:true}).fill(source.started_at_to);
          const result = await action('export',exporter.getByRole('button',{name:'Export complete records',exact:true}));
          assert.ok(result.receipt.exported_records > 0 && result.receipt.exported_spans > 0);
          if (source.expected_events) { assert.equal(source.expected_events,21); assert.equal(result.receipt.exported_spans,21); }
          else assert.equal(result.receipt.exported_spans,5,'retained native source spans');
          for (const id of result.receipt.trace_ids) traceIds.add(id);
          await settledReceipt(result.receipt);
          await screenshot(`02-export-${source.application_type}`);
          receipt.exports.push({viewport:name,instance_id:first.instance_id,application_id:source.application_id,...result.receipt}); checkpoint();
          await close('Export application logs');
        }
        const remote = await waitRemote([...traceIds], receipt.exports.filter(item => item.viewport === name).reduce((sum, item) => sum + item.exported_spans, 0));
        await row(first.instance_id).getByRole('button',{name:'Import',exact:true}).click();
        const importer=dialog('Import into AgentLogs').locator('form');
        await chooseApplication({page,form:importer,label:'Target AgentLogs application',applicationId:target.application_id,deadlineMs:args.deadlineMs});
        await importer.getByLabel('Trace IDs (one per line or comma-separated)',{exact:true}).fill([...traceIds].join('\n'));
        const imported = await action('import',importer.getByRole('button',{name:'Import complete traces',exact:true}));
        assert.equal(imported.receipt.accepted_events + imported.receipt.duplicate_events,26);
        receipt.imports.push({viewport:name,...imported.receipt});
        await settledReceipt(imported.receipt);
        await screenshot('04-import-receipt');
        const repeat = await action('import',importer.getByRole('button',{name:'Import complete traces',exact:true}));
        assert.equal(repeat.receipt.accepted_events,0);
        assert.equal(repeat.receipt.duplicate_events,26);
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
        assert.ok(fieldIds.length>0,'actual label-associated form inputs');
        assert.equal(new Set(fieldIds).size, fieldIds.length, 'unique form field IDs');
        receipt.checks.push({ name:`${name} unique form IDs and source/target/date label associations`, status:'pass' });
        await close('Import into AgentLogs');
        // Full reload plus successful per-row test proves persisted selected credentials.
        await page.reload({waitUntil:'domcontentloaded'}); await row(first.instance_id).waitFor();
        await row(first.instance_id).getByRole('button',{name:'Edit',exact:true}).click();
        assert.equal(await dialog('Instance').getByLabel('Service URL',{exact:true}).inputValue(),first.base_url);
        assert.equal(await dialog('Instance').getByLabel('Public key',{exact:true}).inputValue(),'');
        assert.equal(await dialog('Instance').getByLabel('Secret key',{exact:true}).inputValue(),'');
        await close('Instance');
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
      } catch(error) {
        const output=path.join(args.outDir,`failure-${name}-private.png`);
        await context.pages()[0]?.screenshot({path:output,fullPage:true,animations:'disabled'}).catch(()=>{});
        const accessibility=path.join(args.outDir,`failure-${name}-accessibility-private.txt`);
        const snapshot=await context.pages()[0]?.locator('body').ariaSnapshot({timeout:5000}).catch(()=>undefined);
        if(snapshot)fs.writeFileSync(accessibility,snapshot,{mode:0o600});
        receipt.browser_failure={viewport:name,error:error.message,screenshot:output,accessibility};checkpoint();throw error;
      } finally { try { await context.close(); } finally { await browser.close(); browser=null; } }
    }
    return [...traceIds];
  } finally {
    try { if(browser) await browser.close(); }
    finally {
      try { if(owner) { await owner.dispose(); receipt.browser_session_revoked=true; } }
      finally { fs.rmSync(storage,{force:true}); receipt.browser_storage_removed=!fs.existsSync(storage); checkpoint(); }
    }
  }
}
module.exports={capture};
