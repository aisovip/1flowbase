'use strict';
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');
const { execFileSync } = require('node:child_process');
const { createOwnedGateway } = require('../native-recovery-acceptance/gateway-owner.cjs');
const { createMock } = require('./mock.cjs');

async function main() {
  const out = path.resolve(process.env.HANDOFF_ARTIFACT_DIR || 'tmp/test-governance/gateway-target-handoff/mock');
  fs.mkdirSync(out, { recursive: true });
  let gateway;
  const responses = [];
  const sessionId = crypto.randomUUID();
  const oldPackage = process.env.HANDOFF_OLD_PACKAGE;
  const newPackage = process.env.HANDOFF_NEW_PACKAGE;
  if (!oldPackage || !newPackage) throw new Error('HANDOFF_OLD_PACKAGE and HANDOFF_NEW_PACKAGE are required');
  const mock = await createMock();
  try {
    gateway = await createOwnedGateway({ upstreamBaseUrl: mock.baseUrl, artifactRoot: path.join(out, 'service-logs'),
      pluginPackage: oldPackage, additionalPluginPackages: [newPackage] });
    async function request(kind, history) {
      const metadata = { thread_id: sessionId, turn_id: crypto.randomUUID(), request_kind: kind,
        ...(kind === 'compaction' ? { compaction: { implementation: 'responses', trigger: 'manual',
          reason: 'user_requested', phase: 'standalone_turn' } } : {}) };
      const response = await fetch(gateway.target.gateway.responses_url, {
        method: 'POST', headers: { 'content-type': 'application/json', authorization: gateway.target.gateway.authorization,
          'session-id': sessionId, 'x-codex-turn-metadata': JSON.stringify(metadata) },
        body: JSON.stringify({ model: 'gpt-6-luna', reasoning: { effort: 'max' }, stream: true,
          store: false, input: history }), signal: AbortSignal.timeout(60_000) });
      const raw = await response.text();
      const events = raw.split(/\r?\n\r?\n/).flatMap(block => {
        const data = block.split(/\r?\n/).filter(line => line.startsWith('data:')).map(line => line.slice(5).trim()).join('\n');
        if (!data || data === '[DONE]') return [];
        return [JSON.parse(data)];
      });
      responses.push({ kind, status: response.status, events });
      assert.equal(response.status, 200);
      assert.ok(events.some(event => event.type === 'response.completed'), `${kind} must complete: ${raw.slice(0, 1200)}`);
      assert.ok(!events.some(event => event.type === 'response.failed'));
    }
    const history = [{ role: 'user', content: 'Remember HANDOFF_SENTINEL.' }];
    await request('turn', history);
    const uploaded = await gateway.ownerClient.uploadPackage(newPackage);
    const switched = await gateway.ownerClient.write('/api/console/settings/model-providers/plugins/families/openai/switch-version',
      'POST', { installation_id: uploaded.installation.id });
    assert.equal(switched.data.status, 'succeeded');
    history.push({ role: 'assistant', content: 'HANDOFF_SENTINEL' });
    await request('compaction', history);
    await request('turn', [{ role: 'user', content: 'Summary: HANDOFF_SENTINEL. Return the retained sentinel.' }]);
    const sql = "SELECT jsonb_build_object('plugin',plugin_id,'status',status,'model',upstream_model_id,'effort',reasoning_effort) FROM model_provider_request_logs ORDER BY started_at;";
    const logs = execFileSync('docker', ['exec', 'docker-db-1', 'psql', '-U', 'postgres', '-d', gateway.databaseName, '-At', '-c', sql],
      { encoding: 'utf8', maxBuffer: 16384 }).trim().split('\n').filter(Boolean).map(JSON.parse);
    assert.equal(logs.length, 3);
    assert.notEqual(logs[0].plugin, logs[1].plugin, 'real published request must select a new plugin target');
    assert.equal(logs[1].plugin, logs[2].plugin);
    assert.ok(logs.every(log => log.status === 'succeeded' && log.model === 'gpt-6-luna' && log.effort === 'max'));
    assert.equal(mock.requests.length, 3);
    const evidence = { pass: true, controlled_mock: true, sessionId, responses, logs, upstream: mock.requests };
    fs.writeFileSync(path.join(out, 'evidence.json'), JSON.stringify(evidence, null, 2));
    console.log(JSON.stringify({ pass: true, plugins: logs.map(log => log.plugin), artifact: out }));
  } finally {
    if (responses.length) fs.writeFileSync(path.join(out, 'responses.json'), JSON.stringify(responses, null, 2));
    await gateway?.close();
    await mock.close();
  }
}
main().catch(error => { console.error(error.stack); process.exitCode = 1; });
