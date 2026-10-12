'use strict';
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');
const { spawn, execFileSync } = require('node:child_process');
const { createOwnedGateway } = require('../native-recovery-acceptance/gateway-owner.cjs');
const { Rpc } = require('../worker-lifecycle-acceptance/rpc.cjs');
const { codexEnvironment } = require('../cli-smoke/environment');

async function main() {
  const required = key => { if (!process.env[key]) throw new Error(`Missing ${key}`); return process.env[key]; };
  const oldPackage = required('HANDOFF_OLD_PACKAGE');
  const newPackage = required('HANDOFF_NEW_PACKAGE');
  const content = fs.readFileSync(required('HANDOFF_KEY_FILE'), 'utf8').trim();
  const matches = [...new Set(content.match(/\bsk-[A-Za-z0-9_-]+/g) || [])];
  const key = matches.length === 1 ? matches[0] : (!content.includes('\n') && !matches.length ? content : null);
  if (!key) throw new Error('Gateway key file must contain one unambiguous credential');
  const upstream = required('HANDOFF_UPSTREAM');
  if (new URL(upstream).port === '7600') throw new Error('7600 is a protected observation listener');
  const out = path.resolve(required('HANDOFF_ARTIFACT_DIR'));
  fs.mkdirSync(out, { recursive: true, mode: 0o700 });
  const home = path.join(out, 'client-home');
  fs.mkdirSync(home, { mode: 0o700 });
  const catalog = fs.readFileSync(required('HANDOFF_MODEL_CATALOG'), 'utf8');
  assert.ok(JSON.parse(catalog).models?.some(model => model.slug === 'gpt-6-luna'));
  fs.writeFileSync(path.join(home, 'catalog.json'), catalog, { mode: 0o600 });
  const sentinel = `HANDOFF_LIVE_${crypto.randomUUID()}`;
  const evidence = { model: 'gpt-6-luna', reasoning_effort: 'max', upstream, sentinel,
    candidate_sha: execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8', maxBuffer: 4096 }).trim(),
    codex_version: execFileSync('codex', ['--version'], { encoding: 'utf8', maxBuffer: 4096 }).trim(),
    started_at: new Date().toISOString(), pass: false };
  let gateway, child;
  try {
    gateway = await createOwnedGateway({ upstreamBaseUrl: upstream, upstreamApiKey: key,
      artifactRoot: path.join(out, 'service-logs'), pluginPackage: oldPackage, additionalPluginPackages: [newPackage] });
    const env = codexEnvironment(process.env, { home, config: home }, gateway.target.gateway.authorization.slice(7));
    const redact = value => JSON.stringify(value).split(key).join('[REDACTED]')
      .split(gateway.target.gateway.authorization.slice(7)).join('[REDACTED]');
    fs.writeFileSync(path.join(home, 'config.toml'), `model = "gpt-6-luna"\nmodel_provider = "candidate"\nmodel_reasoning_effort = "max"\nmodel_catalog_json = ${JSON.stringify(path.join(home, 'catalog.json'))}\napproval_policy = "never"\nsandbox_mode = "read-only"\n[model_providers.candidate]\nname = "Gateway target handoff acceptance"\nbase_url = ${JSON.stringify(`${gateway.baseUrl}/v1`)}\nenv_key = "ONEFLOWBASE_APPLICATION_API_KEY"\nwire_api = "responses"\nsupports_websockets = true\n`, { mode: 0o600 });
    child = spawn('codex', ['app-server', '--stdio'], { cwd: home, env, stdio: ['pipe', 'pipe', 'pipe'] });
    const events = [];
    const record = value => {
      const safe = JSON.parse(redact(value));
      events.push(safe);
      fs.appendFileSync(path.join(out, 'timeline.jsonl'), `${JSON.stringify(safe)}\n`, { mode: 0o600 });
    };
    child.stderr.on('data', data => record({ kind: 'client/stderr', text: data.toString() }));
    const rpc = new Rpc(child, record);
    await rpc.request('initialize', { clientInfo: { name: 'target_handoff_acceptance', version: '1' },
      capabilities: { experimentalApi: true } });
    rpc.send({ method: 'initialized', params: {} });
    const thread = await rpc.request('thread/start', { model: 'gpt-6-luna', modelProvider: 'candidate', cwd: home,
      approvalPolicy: 'never', sandbox: 'read-only', ephemeral: false });
    const threadId = thread.thread.id;
    evidence.thread_id = threadId;
    async function turn(text) {
      const started = await rpc.request('turn/start', { threadId, model: 'gpt-6-luna', effort: 'max',
        input: [{ type: 'text', text, text_elements: [] }] });
      const terminal = await rpc.wait(event => event.method === 'turn/completed' && event.params.threadId === threadId
        && event.params.turn.id === started.turn.id, 240_000);
      assert.equal(terminal.params.turn.status, 'completed', JSON.stringify(terminal.params.turn));
      return started.turn.id;
    }
    evidence.seed_turn = await turn(`Remember this exact identifier for a later recall test: ${sentinel}. Reply only STORED. Do not use tools or read files.`);
    const uploaded = await gateway.ownerClient.uploadPackage(newPackage);
    const switched = await gateway.ownerClient.write('/api/console/settings/model-providers/plugins/families/openai/switch-version',
      'POST', { installation_id: uploaded.installation.id });
    assert.equal(switched.data.status, 'succeeded');
    evidence.switched_at = new Date().toISOString();
    const since = rpc.history.length;
    await rpc.request('thread/compact/start', { threadId });
    const compact = await rpc.wait(event => event.method === 'item/completed' && event.params.threadId === threadId
      && event.params.item.type === 'contextCompaction', 240_000, since);
    const compactTerminal = await rpc.wait(event => event.method === 'turn/completed' && event.params.threadId === threadId
      && event.params.turn.id === compact.params.turnId, 240_000, since);
    assert.equal(compactTerminal.params.turn.status, 'completed');
    evidence.compact_turn = compact.params.turnId;
    const resumed = await turn('Return the exact identifier I asked you to remember earlier. Do not read files, use tools or guess it.');
    const answer = events.filter(event => event.method === 'item/completed' && event.params?.turnId === resumed
      && event.params.item.type === 'agentMessage').map(event => event.params.item.text || '').join('\n');
    assert.ok(answer.includes(sentinel), 'compacted history must retain the exact prior identifier');
    evidence.resumed_turn = resumed;
    evidence.recalled_sentinel = true;
    const sql = "SELECT jsonb_build_object('plugin',p.plugin_id,'status',p.status,'model',p.upstream_model_id,'effort',p.reasoning_effort,'kind',f.log_context->>'request_kind') FROM model_provider_request_logs p JOIN flow_runs f ON f.id=p.flow_run_id ORDER BY p.started_at;";
    evidence.provider_logs = execFileSync('docker', ['exec', 'docker-db-1', 'psql', '-U', 'postgres', '-d', gateway.databaseName, '-At', '-c', sql],
      { encoding: 'utf8', maxBuffer: 32768 }).trim().split('\n').filter(Boolean).map(JSON.parse);
    const compactLog = evidence.provider_logs.find(log => log.kind === 'compaction' && log.status === 'succeeded');
    const seedLog = evidence.provider_logs.find(log => log.kind === 'turn' && log.status === 'succeeded');
    assert.ok(compactLog && seedLog);
    assert.notEqual(compactLog.plugin, seedLog.plugin);
    assert.ok(evidence.provider_logs.every(log => log.model === 'gpt-6-luna' && log.effort === 'max'));
    assert.ok(!evidence.provider_logs.some(log => log.status === 'failed'));
    evidence.pass = true;
    console.log(JSON.stringify({ pass: true, compact_turn: evidence.compact_turn, recalled_sentinel: true, artifact: out }));
  } finally {
    if (child) {
      const exited = new Promise(resolve => child.exitCode !== null || child.signalCode !== null ? resolve() : child.once('exit', resolve));
      child.stdin.end();
      child.kill('SIGTERM');
      const timer = setTimeout(() => child.kill('SIGKILL'), 5000);
      await exited;
      clearTimeout(timer);
    }
    evidence.finished_at = new Date().toISOString();
    fs.writeFileSync(path.join(out, 'evidence.json'), JSON.stringify(evidence, null, 2), { mode: 0o600 });
    await gateway?.close();
  }
}
main().catch(error => { console.error(error.stack); process.exitCode = 1; });
