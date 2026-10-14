'use strict';
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');
const { parseArgs } = require('node:util');
const { spawn, execFileSync } = require('node:child_process');
const { loadRootCredentials, openTemporaryOwnerSession } = require('../page-debug/auth');
const { syntheticBatch } = require('./fixtures.cjs');
const { capture } = require('./browser.cjs');
const root = path.resolve(__dirname, '../../..');
const options = {
  'api-base-url': { type: 'string' }, 'web-base-url': { type: 'string' },
  'credentials-root': { type: 'string' }, 'langfuse-private-file': { type: 'string' },
  'openlit-module-root': { type: 'string' }, 'out-dir': { type: 'string' },
  'gateway-key-file': { type: 'string' }, 'langfuse-storage-private-file': { type: 'string' },
  'deadline-ms': { type: 'string', default: '120000' }
};
function argumentsForRun() {
  const { values } = parseArgs({ options });
  for (const required of ['api-base-url', 'web-base-url', 'credentials-root', 'langfuse-private-file', 'openlit-module-root', 'langfuse-storage-private-file', 'out-dir'])
    assert.ok(values[required], `missing --${required}`);
  const args = Object.fromEntries(Object.entries(values).map(([key, value]) => [key.replace(/-([a-z])/g, (_, char) => char.toUpperCase()), value]));
  for (const key of ['apiBaseUrl', 'webBaseUrl']) {
    const url = new URL(args[key]);
    assert.ok(['http:', 'https:'].includes(url.protocol) && !url.username && !url.password && !url.search && !url.hash, 'explicit origin required');
    assert.ok(url.port !== '7600', 'protected observation service is outside this scenario');
    args[key] = url.origin;
  }
  args.outDir = path.resolve(args.outDir);
  const relative = path.relative(path.join(root, 'tmp/test-governance'), args.outDir);
  assert.ok(relative && !relative.startsWith('..') && !path.isAbsolute(relative), 'out-dir must be within candidate tmp/test-governance');
  args.deadlineMs = Number(args.deadlineMs);
  assert.ok(Number.isSafeInteger(args.deadlineMs) && args.deadlineMs > 0, 'positive readiness deadline required');
  return args;
}
function fail(message) { const error = new Error(message); error.safe_reason = message; throw error; }
function receiptCounts(value, accepted, duplicates) {
  assert.equal(value.accepted_events, accepted, 'accepted count');
  assert.equal(value.duplicate_events, duplicates, 'duplicate count');
  assert.ok(Array.isArray(value.record_ids) && value.record_ids.every(id => typeof id === 'string'), 'record IDs');
}
const pause = () => new Promise(resolve => setTimeout(resolve, 1000));

async function run(args) {
  fs.mkdirSync(path.join(args.outDir, 'private'), { recursive: true, mode: 0o700 });
  const receipt = { source_sha: execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8', maxBuffer: 4096 }).trim(), checks: [], created_application_ids: [], trace_ids: [], screenshots: [], errors: [], cleanup: {} };
  let owner, stage = 'private inputs';
  const keys = [];
  const check = (name, details = {}) => receipt.checks.push({ name, status: 'pass', ...details });
  const request = async (method, route, body, token) => {
    const response = await fetch(args.apiBaseUrl + route, { method, redirect: 'error', signal: AbortSignal.timeout(30000),
      headers: { 'content-type': 'application/json', ...(token ? { Authorization: `Bearer ${token}` } : { cookie: owner.cookie, 'x-csrf-token': owner.csrfToken }) },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
    if (!response.ok) fail(`HTTP ${response.status}`);
    if (response.status === 204) return null;
    const payload = await response.json();
    if (route.endsWith('/otlp/export')) return payload;
    assert.ok(payload && Object.hasOwn(payload, 'data'), 'ApiSuccess envelope');
    return payload.data;
  };
  const recordPath = (app, id) => `/api/console/applications/${app}/logs/records/${id}`;
  const verifyRecords = async (app, ids, batch, name) => {
    assert.equal(ids.length, 3, 'three source tasks retained');
    for (const id of ids) {
      const overview = await request('GET', recordPath(app, id));
      const expected = batch.events.filter(event => event.source_task_id === overview.source_task_id);
      assert.equal(expected.length, 7, 'source task identity');
      assert.equal(overview.source_kind, 'imported');
      assert.equal(overview.native_run_id, null, 'no fabricated native run');
      assert.equal(overview.source_id, batch.source_id);
      assert.equal(overview.source_client, batch.source_client);
      assert.equal(overview.source_session_id, expected[0].source_session_id);
      assert.equal(overview.requested_model_id, 'synthetic-fixture-model');
      const usage = expected.find(event => event.kind === 'usage').usage;
      for (const field of ['input_tokens', 'output_tokens', 'total_tokens']) assert.equal(overview[field], usage[field], field);
      for (const kind of ['user', 'assistant']) {
        const content = expected.find(event => event.kind === kind).content;
        assert.ok(overview.messages.some(message => message.role === kind && message.content === content), 'source messages');
      }
      const steps = []; let cursor; const seenCursors = new Set();
      do {
        const query = new URLSearchParams({ limit: cursor === undefined ? '50' : '100' });
        if (cursor !== undefined) query.set('cursor', String(cursor));
        const page = await request('GET', `${recordPath(app, id)}/client-trajectory?${query}`);
        steps.push(...page.items); cursor = page.next_cursor ?? undefined;
        if (cursor !== undefined) { assert.ok(!seenCursors.has(String(cursor)), 'trajectory cursor must advance'); seenCursors.add(String(cursor)); }
      } while (cursor !== undefined);
      assert.equal(steps.length, expected.length, 'all event identities retained');
      for (const event of expected) {
        const step = steps.find(value => value.item_id === event.event_id);
        assert.ok(step, 'source event identity retained');
        assert.equal(step.flow_run_id, null);
        assert.equal(step.category, event.kind);
        const raw = await request('GET', `${recordPath(app, id)}/client-trajectory/${step.id}?limit=8&section=raw`);
        assert.equal(raw.items.length, 1, 'single original source fact');
        assert.deepEqual(raw.items[0].value, event, 'exact original event, context, usage and raw');
      }
    }
    check(name, { application_id: app, record_ids: ids, events: batch.events.length });
  };
  try {
    const langfuse = JSON.parse(fs.readFileSync(args.langfusePrivateFile, 'utf8'));
    for (const field of ['base_url', 'public_key', 'secret_key', 'project_id', 'email', 'password']) assert.equal(typeof langfuse[field], 'string', `private ${field}`);
    langfuse.base_url = langfuse.base_url.replace(/\/$/, '');
    const credentials = loadRootCredentials({ repoRoot: args.credentialsRoot });
    stage = 'temporary owner';
    owner = await openTemporaryOwnerSession({ apiBaseUrl: args.apiBaseUrl, ...credentials });
    const suffix = crypto.randomUUID().slice(0, 8);
    stage = 'create demo applications';
    const apps = [];
    for (const label of ['Agent Logs 互通来源演示', 'Agent Logs OTLP 文件导入演示', 'Agent Logs Langfuse 导入演示']) {
      const app = await request('POST', '/api/console/applications', { application_type: 'agent_logs', name: `${label} ${suffix}`, description: 'Synthetic exchange acceptance fixture; created by the scenario', icon: null, icon_type: null, icon_background: null });
      receipt.created_application_ids.push(app.id); apps.push(app.id);
      await request('PATCH', `/api/console/applications/${app.id}/api-status`, { api_enabled: true });
      const key = await request('POST', `/api/console/applications/${app.id}/api-keys`, { name: `exchange-scenario-${suffix}`, expires_at: null });
      assert.ok(key.token && key.id, 'new application key'); keys.push({ application_id: app.id, id: key.id, token: key.token });
    }
    check(stage, { application_ids: apps });
    stage = 'canonical synthetic events';
    const batch = syntheticBatch();
    const ingested = await request('POST', '/api/logs/v1/events', batch, keys[0].token);
    receiptCounts(ingested, batch.events.length, 0);
    await verifyRecords(apps[0], ingested.record_ids, batch, stage);
    stage = 'OTLP export and file import';
    const otlp = await request('POST', `/api/console/applications/${apps[0]}/logs/otlp/export`, { record_ids: ingested.record_ids });
    assert.ok(Array.isArray(otlp.resourceSpans), 'actual OTLP JSON resourceSpans');
    const otlpSpans = otlp.resourceSpans.flatMap(resource => resource.scopeSpans.flatMap(scope => scope.spans));
    assert.equal(otlpSpans.length, batch.events.length, 'all source events exported as spans');
    assert.ok(otlpSpans.every(span => typeof span.traceId === 'string' && span.traceId), 'OTLP span Trace IDs');
    const otlpTraceIds = [...new Set(otlpSpans.map(span => span.traceId))].sort();
    const expectedTraceCount = new Set(batch.events.map(event => event.source_session_id)).size;
    assert.equal(otlpTraceIds.length, expectedTraceCount, 'one trace per source session, task hierarchy retained as spans');
    const file = path.join(args.outDir, 'synthetic-otlp.json'); fs.writeFileSync(file, JSON.stringify(otlp, null, 2));
    const fromFile = JSON.parse(fs.readFileSync(file, 'utf8'));
    const imported = await request('POST', `/api/console/applications/${apps[1]}/logs/otlp/import`, { otlp: fromFile });
    receiptCounts(imported, batch.events.length, 0);
    await verifyRecords(apps[1], imported.record_ids, batch, stage);
    const repeated = await request('POST', `/api/console/applications/${apps[1]}/logs/otlp/import`, { otlp: fromFile });
    receiptCounts(repeated, 0, batch.events.length);
    assert.deepEqual([...repeated.record_ids].sort(), [...imported.record_ids].sort());
    check('OTLP duplicate import', { ...repeated });
    stage = 'Langfuse multi-record export';
    const lfCredentials = { base_url: langfuse.base_url, public_key: langfuse.public_key, secret_key: langfuse.secret_key };
    const exported = await request('POST', `/api/console/applications/${apps[0]}/logs/langfuse/export`, { record_ids: ingested.record_ids, ...lfCredentials });
    assert.equal(exported.exported_records, 3);
    assert.equal(exported.exported_spans, batch.events.length);
    assert.equal(exported.trace_ids.length, expectedTraceCount, 'Langfuse traces match source sessions');
    assert.deepEqual([...new Set(exported.trace_ids)].sort(), otlpTraceIds, 'Langfuse and downloaded OTLP expose identical Trace IDs');
    receipt.trace_ids = exported.trace_ids;
    check(stage, exported);
    stage = 'Langfuse worker persisted observations';
    const deadline = Date.now() + args.deadlineMs;
    let observationsReady = false;
    do {
      const observed = new Map();
      for (const traceId of exported.trace_ids) {
        let cursor;
        const seenCursors = new Set();
        do {
          // A small transport page exercises pagination; it does not cap business data.
          const query = new URLSearchParams({ traceId, limit: '2', fields: 'basic' });
          if (cursor !== undefined) query.set('cursor', cursor);
          const response = await fetch(`${langfuse.base_url}/api/public/v2/observations?${query}`, { headers: { Authorization: 'Basic ' + Buffer.from(`${langfuse.public_key}:${langfuse.secret_key}`).toString('base64') }, signal: AbortSignal.timeout(10000), redirect: 'error' });
          if (!response.ok) fail(`Langfuse readiness HTTP ${response.status}`);
          const data = await response.json(); assert.ok(Array.isArray(data.data), 'Langfuse v2 observations');
          for (const observation of data.data) if (observation.traceId === traceId) observed.set(observation.id, observation.traceId);
          cursor = data.meta?.nextCursor ?? undefined;
          if (cursor !== undefined) {
            assert.ok(typeof cursor === 'string' && cursor && !seenCursors.has(cursor), 'Langfuse cursor must advance');
            seenCursors.add(cursor);
          }
          if (Date.now() >= deadline) fail('Langfuse observations deadline');
        } while (cursor !== undefined);
      }
      observationsReady = observed.size >= exported.exported_spans && exported.trace_ids.every(id => [...observed.values()].includes(id));
      if (observationsReady) break;
      await pause();
    } while (Date.now() < deadline);
    assert.ok(observationsReady, 'Langfuse observations deadline'); check(stage, { trace_ids: exported.trace_ids });
    stage = 'Langfuse multi-trace import';
    const lfImported = await request('POST', `/api/console/applications/${apps[2]}/logs/langfuse/import`, { trace_ids: exported.trace_ids, ...lfCredentials });
    receiptCounts(lfImported, batch.events.length, 0);
    await verifyRecords(apps[2], lfImported.record_ids, batch, stage);
    const lfRepeat = await request('POST', `/api/console/applications/${apps[2]}/logs/langfuse/import`, { trace_ids: exported.trace_ids, ...lfCredentials });
    receiptCounts(lfRepeat, 0, batch.events.length);
    assert.deepEqual([...lfRepeat.record_ids].sort(), [...lfImported.record_ids].sort());
    check('Langfuse duplicate import', lfRepeat);
    stage = 'real OpenLIT SDK';
    const prompts = ['Reply with one short sentence explaining 1 plus 1.', 'Reply with one short sentence explaining 2 plus 1.'];
    const messages = [];
    let gatewayKey;
    if (args.gatewayKeyFile) {
      gatewayKey = fs.readFileSync(args.gatewayKeyFile, 'utf8').trim();
      assert.match(gatewayKey, /^sk-[A-Za-z0-9_.-]+$/, 'gateway file must contain exactly one key token');
    }
    for (const [index, prompt] of prompts.entries()) {
      if (!gatewayKey) messages.push({ prompt, answer: `Synthetic OpenLIT SDK answer ${index + 1}.`, model: 'synthetic-fixture-model', input_tokens: 5, output_tokens: 4, source: 'synthetic' });
      else {
        const response = await fetch('http://127.0.0.1:7800/v1/chat/completions', { method: 'POST', headers: { 'content-type': 'application/json', Authorization: `Bearer ${gatewayKey}` }, body: JSON.stringify({ model: 'gpt-6-luna', reasoning_effort: 'max', messages: [{ role: 'user', content: prompt }] }), signal: AbortSignal.timeout(120000), redirect: 'error' });
        if (!response.ok) fail(`gateway HTTP ${response.status}`);
        const completion = await response.json();
        const answer = completion.choices?.[0]?.message?.content;
        assert.ok(typeof answer === 'string' && answer.trim(), 'gateway textual answer');
        const input_tokens = completion.usage?.prompt_tokens, output_tokens = completion.usage?.completion_tokens;
        assert.ok(Number.isSafeInteger(input_tokens) && Number.isSafeInteger(output_tokens), 'gateway real token usage');
        messages.push({ prompt, answer, model: 'gpt-6-luna', input_tokens, output_tokens, source: 'gateway' });
      }
    }
    const sessionIds = [crypto.randomUUID(), crypto.randomUUID()];
    const summaryPath = path.join(args.outDir, 'openlit-summary.json');
    await new Promise((resolve, reject) => {
      const sdkLog = fs.openSync(path.join(args.outDir, 'private', 'openlit-sdk.log'), 'w', 0o600);
      const child = spawn(process.execPath, [path.join(__dirname, 'openlit.cjs')], { stdio: ['pipe', sdkLog, sdkLog], env: { ...process.env, OTEL_TRACES_EXPORTER: 'otlp' } });
      fs.closeSync(sdkLog);
      const timer = setTimeout(() => { child.kill('SIGTERM'); }, args.deadlineMs);
      child.once('error', () => { clearTimeout(timer); reject(new Error('SDK child start failed')); });
      child.once('exit', code => { clearTimeout(timer); code === 0 ? resolve() : reject(new Error('SDK child failed')); });
      child.stdin.on('error', () => {});
      child.stdin.end(JSON.stringify({ moduleRoot: args.openlitModuleRoot, apiBaseUrl: args.apiBaseUrl, key: keys[0].token, sessionIds, messages, summaryPath }));
    });
    const summary = JSON.parse(fs.readFileSync(summaryPath, 'utf8'));
    const until = Date.now() + args.deadlineMs;
    let sdkRecords = [];
    do {
      const page = await request('POST', '/api/console/applications/logs/conversations/query', { application_ids: [apps[0]], page: 1, page_size: 20 });
      sdkRecords = page.items.filter(record => sessionIds.includes(record.source_session_id));
      if (sdkRecords.length === 2) break;
      await pause();
    } while (Date.now() < until);
    assert.equal(sdkRecords.length, 2, 'real SDK session records must arrive');
    for (const [index, sessionId] of sessionIds.entries()) {
      const record = sdkRecords.find(item => item.source_session_id === sessionId);
      const overview = await request('GET', recordPath(apps[0], record.record_id));
      assert.equal(overview.native_run_id, null);
      assert.equal(overview.requested_model_id, messages[index].model);
      assert.equal(overview.input_tokens, messages[index].input_tokens);
      assert.equal(overview.output_tokens, messages[index].output_tokens);
      const events = []; let cursor;
      const seenCursors = new Set();
      do {
        const query = new URLSearchParams({ limit: cursor === undefined ? '50' : '100' });
        if (cursor !== undefined) query.set('cursor', String(cursor));
        const page = await request('GET', `${recordPath(apps[0], record.record_id)}/client-trajectory?${query}`);
        for (const step of page.items) {
          assert.equal(step.flow_run_id, null, 'SDK span must not fabricate native run');
          const section = await request('GET', `${recordPath(apps[0], record.record_id)}/client-trajectory/${step.id}?limit=8&section=raw`);
          for (const item of section.items) events.push(item.value);
        }
        cursor = page.next_cursor ?? undefined;
        if (cursor !== undefined) {
          assert.ok(!seenCursors.has(String(cursor)), 'SDK trajectory cursor must advance');
          seenCursors.add(String(cursor));
        }
      } while (cursor !== undefined);
      assert.ok(events.some(event => event.kind === 'context' && typeof event.content === 'string' && event.content.includes(messages[index].prompt)), 'LLM history prompt retained as Context');
      assert.ok(events.some(event => event.kind === 'assistant' && event.content === messages[index].answer), 'ordinary SDK output retained as Assistant');
      assert.ok(events.every(event => event.kind !== 'task_end' && event.phase !== 'final_answer'), 'SDK span without final declaration must not fabricate TaskEnd or final phase');
      const rawSpans = events.map(event => JSON.stringify(event.raw));
      const inputMessages = JSON.stringify([{ role: 'user', content: messages[index].prompt }]);
      const outputMessages = JSON.stringify([{ role: 'assistant', content: messages[index].answer }]);
      assert.ok(rawSpans.some(raw => raw.includes('gen_ai.input.messages') && raw.includes(JSON.stringify(inputMessages).slice(1, -1))), 'original SDK input message attribute retained in raw span');
      assert.ok(rawSpans.some(raw => raw.includes('gen_ai.output.messages') && raw.includes(JSON.stringify(outputMessages).slice(1, -1))), 'original SDK output message attribute retained in raw span');
    }
    check(stage, { sdk: summary.sdk, version: summary.version, spans: summary.spans, record_ids: sdkRecords.map(record => record.record_id), message_source: gatewayKey ? 'gateway' : 'synthetic' });
    stage = 'browser screenshots';
    const conversationRecord = await request('GET', recordPath(apps[2], lfImported.record_ids[0]));
    const conversationEvents = batch.events.filter(event => event.source_task_id === conversationRecord.source_task_id);
    const conversation = {
      record_id: conversationRecord.record_id,
      user: conversationEvents.find(event => event.kind === 'user').content,
      final_answer: conversationEvents.find(event => event.kind === 'assistant' && event.phase === 'final_answer').content
    };
    await capture({ args, credentials, applicationId: apps[0], recordIds: ingested.record_ids,
      targetApplicationId: apps[2], importedRecordIds: lfImported.record_ids,
      conversation, traceIds: exported.trace_ids, eventCount: batch.events.length, langfuse, receipt });
  } catch (error) {
    // Do not serialize error messages, bodies or SDK diagnostics: these can contain credentials.
    receipt.errors.push({ stage, ...(error.safe_reason ? { reason: error.safe_reason } : {}), error_name: ['AssertionError', 'TypeError', 'SyntaxError'].includes(error.name) ? error.name : 'Error' });
    process.exitCode = 1;
  } finally {
    if (owner) {
      for (const key of keys) {
        try { await request('DELETE', `/api/console/applications/${key.application_id}/api-keys/${key.id}`); }
        catch { receipt.errors.push({ stage: 'revoke scenario API key', application_id: key.application_id }); process.exitCode = 1; }
      }
      try { await owner.dispose(); receipt.cleanup.owner_session_disposed = true; }
      catch { receipt.errors.push({ stage: 'dispose owner session' }); process.exitCode = 1; }
    }
    receipt.cleanup.demo_applications_retained = true;
    receipt.status = receipt.errors.length ? 'failed' : 'passed';
    fs.writeFileSync(path.join(args.outDir, 'runtime-receipt.json'), JSON.stringify(receipt, null, 2), { mode: 0o600 });
    process.stdout.write(JSON.stringify({ status: receipt.status, checks: receipt.checks.length, errors: receipt.errors.length, receipt: path.join(args.outDir, 'runtime-receipt.json') }).slice(0, 2000) + '\n');
  }
}
try { run(argumentsForRun()).catch(() => { process.stderr.write('Scenario setup failed; no credentials printed.\n'); process.exitCode = 1; }); }
catch { process.stderr.write('Invalid scenario arguments; see README.md.\n'); process.exitCode = 1; }
