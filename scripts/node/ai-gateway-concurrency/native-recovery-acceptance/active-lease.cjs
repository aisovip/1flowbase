'use strict';
// Cross-transport regression: an old admission receipt with a live execution
// lease must attach after a client disconnect, without re-consuming the tool.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const cp = require('node:child_process');
const assert = require('node:assert/strict');
const { createOwnedGateway } = require('./gateway-owner.cjs');
const { createActiveLeaseMock } = require('./active-lease-mock.cjs');
const { decodeSseChunks } = require('../protocol-oracle/decoder');

async function bounded(promise, label) {
  let timer;
  try {
    return await Promise.race([promise, new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(label)), 30_000); })]);
  } finally { clearTimeout(timer); }
}

function callbackRoundIdentity(sql) {
  return JSON.parse(sql(`select jsonb_build_object(
    'callback_task_id', a.callback_task_id,
    'markers', coalesce((select jsonb_agg(jsonb_build_object(
      'sequence', e.sequence, 'round', p.value->>'response_round_id',
      'generation', p.value->>'stream_generation_id') order by e.sequence)
      from runtime_events e cross join lateral
        (select runtime_event_original_payload(e.payload,e.raw_json_payloads,e.flow_run_id) as value) p
      where e.flow_run_id=a.flow_run_id and e.event_type='flow_started'
        and p.value->>'response_round_id'=a.callback_task_id::text), '[]'::jsonb),
    'anchor_generations', coalesce((select jsonb_agg(p.value->>'stream_generation_id')
      from runtime_events e cross join lateral
        (select runtime_event_original_payload(e.payload,e.raw_json_payloads,e.flow_run_id) as value) p
      where e.flow_run_id=a.flow_run_id and e.event_type='runtime_stream_opened'), '[]'::jsonb))
    from flow_run_callback_resume_attempts a;`));
}

function assertCallbackRoundIdentity(identity) {
  assert.equal(identity.markers.length, 1, 'Original WS resume must persist exactly one real callback round marker');
  assert.equal(identity.markers[0].round, identity.callback_task_id);
  assert.ok(identity.markers[0].generation, 'Callback marker must carry its real generation identity');
  assert.ok(identity.anchor_generations.includes(identity.markers[0].generation), 'Marker generation must belong to a committed anchor');
}

async function run() {
  const artifactRoot = path.resolve(process.env.NRA_ARTIFACT_DIR || 'tmp/test-governance/callback-active-lease');
  fs.mkdirSync(artifactRoot, { recursive: true });
  const mock = await createActiveLeaseMock();
  let gateway, socket, watchdog, outputTimer, releaseError, sql;
  const evidence = { controlledMock: true, model: 'gpt-6-luna', effort: 'max',
    ageInjection: 'Only isolated fixture admission timestamp is aged by 6 minutes; execution lease remains live.' };
  const save = () => fs.writeFileSync(path.join(artifactRoot, 'receipt.json'), JSON.stringify(evidence, null, 2));
  try {
    gateway = await createOwnedGateway({ upstreamBaseUrl: mock.baseUrl, artifactRoot: path.join(artifactRoot, 'service'),
      responsesTransportPolicy: 'inherit' });
    const pnpm = path.join(process.env.NRA_WEB_REPO || path.resolve(__dirname, '../../../..'), 'web/node_modules/.pnpm');
    const installed = fs.readdirSync(pnpm).find(name => /^ws@8\./u.test(name));
    const { WebSocket } = require(path.join(pnpm, installed, 'node_modules/ws'));
    const session = crypto.randomUUID();
    socket = new WebSocket(gateway.target.gateway.websocket_url, { headers: {
      authorization: gateway.target.gateway.authorization, 'session-id': session,
      'openai-beta': 'responses_websockets=2026-02-06',
    } });
    const events = [];
    let onEvent;
    let socketError;
    socket.on('message', data => { const event = JSON.parse(data.toString()); events.push(event); onEvent?.(event); });
    socket.on('error', error => { socketError = error; });
    watchdog = setTimeout(() => socket.terminate(), 90_000);
    await bounded(new Promise((resolve, reject) => { socket.once('open', resolve); socket.once('error', reject); }), 'WebSocket did not open');
    const body = { model: 'gpt-6-luna', reasoning: { effort: 'max' }, stream: true, store: false,
      input: [{ role: 'user', content: [{ type: 'input_text', text: 'Controlled callback lease fixture.' }] }],
      tools: [{ type: 'function', name: 'fixture', description: 'Fixture only',
        parameters: { type: 'object', properties: {}, additionalProperties: false }, strict: true }] };
    const seed = new Promise((resolve, reject) => {
      onEvent = event => {
        if (event.type === 'response.completed') resolve(event);
        else if (event.type === 'error' || event.type === 'response.failed') reject(new Error(JSON.stringify(event)));
      };
    });
    socket.send(JSON.stringify({ ...body, type: 'response.create' }));
    const completed = await bounded(seed, 'Seed did not complete');
    const call = events.find(event => event.type === 'response.output_item.done' && event.item?.call_id)?.item;
    assert.ok(call, 'Seed must return a committed tool call');
    onEvent = undefined;
    const followup = { ...body, previous_response_id: completed.response.id,
      input: [{ type: 'function_call_output', call_id: call.call_id, output: 'fixture-result-once' }] };
    socket.send(JSON.stringify({ ...followup, type: 'response.create' }));
    await bounded(mock.continuing, 'Continuation did not start');
    sql = query => cp.execFileSync('docker', ['exec', 'docker-db-1', 'psql', '-U', 'postgres',
      '-d', gateway.databaseName, '-At', '-v', 'ON_ERROR_STOP=1', '-c', query], { encoding: 'utf8', maxBuffer: 1024 * 1024 }).trim();
    sql("update flow_run_callback_resume_attempts set updated_at = now() - interval '6 minutes' where status = 'processing';");
    evidence.before = JSON.parse(sql("select jsonb_build_object('attempt_status',a.status,'attempt_age_seconds',extract(epoch from now()-a.updated_at),'claim_status',c.status,'lease_live',c.lease_expires_at>now(),'callback_status',t.status) from flow_run_callback_resume_attempts a join flow_run_callback_tasks t on t.id=a.callback_task_id join flow_run_resume_claims c on c.callback_task_id=a.callback_task_id;"));
    assert.equal(evidence.before.lease_live, true);
    assert.equal(evidence.before.callback_status, 'completed');
    let waitingForIdentity = true;
    try {
      evidence.roundIdentityBefore = await bounded((async () => {
        while (waitingForIdentity) {
          const identity = callbackRoundIdentity(sql);
          if (identity.markers.length) return identity;
          await new Promise(resolve => setTimeout(resolve, 50));
        }
      })(), 'Original WS resume did not persist its real callback round identity');
    } finally { waitingForIdentity = false; }
    assertCallbackRoundIdentity(evidence.roundIdentityBefore);
    save();
    socket.terminate();
    await new Promise(resolve => socket.readyState === WebSocket.CLOSED ? resolve() : socket.once('close', resolve));
    assert.equal(socketError, undefined);
    // An attached response follows the durable round terminal. Keep the
    // provider live while the retry is in flight, then finish the original
    // invocation; waiting for retry headers before this would deadlock the fixture.
    outputTimer = setTimeout(() => {
      try {
        evidence.during = JSON.parse(sql("select jsonb_build_object('attempt_status',a.status,'claim_status',c.status,'lease_live',c.lease_expires_at>now()) from flow_run_callback_resume_attempts a join flow_run_resume_claims c on c.callback_task_id=a.callback_task_id;"));
        assert.equal(evidence.during.attempt_status, 'processing');
        assert.equal(evidence.during.claim_status, 'processing');
        assert.equal(evidence.during.lease_live, true);
      } catch (error) { releaseError = error; }
      mock.release();
      evidence.providerReleasedUtc = new Date().toISOString();
      save();
    }, 2000);
    const retry = await fetch(gateway.target.gateway.responses_url, {
      method: 'POST', headers: { 'content-type': 'application/json', authorization: gateway.target.gateway.authorization,
        'session-id': session }, body: JSON.stringify(followup), signal: AbortSignal.timeout(45_000),
    });
    evidence.httpStatus = retry.status;
    if (retry.status !== 200) { evidence.body = await retry.text(); save(); }
    assert.equal(retry.status, 200, 'Live callback lease must attach, even when admission is older than five minutes');
    const reader = retry.body.getReader();
    const chunks = [];
    let raw = '';
    try {
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        chunks.push(Buffer.from(value));
        raw = Buffer.concat(chunks).toString();
        evidence.receivedEvents = decodeSseChunks(chunks).filter(record => !record.done).map(record => record.data);
        save();
        if (evidence.receivedEvents.some(event => ['response.completed', 'response.failed', 'error'].includes(event.type))) break;
      }
    } finally { await reader.cancel().catch(() => {}); }
    if (releaseError) throw releaseError;
    assert.ok(evidence.during, 'The original invocation must remain live until controlled release');
    evidence.events = decodeSseChunks([Buffer.from(raw)]).filter(record => !record.done).map(record => record.data);
    assert.equal(evidence.events.filter(event => event.type === 'response.completed').length, 1);
    assert.ok(!evidence.events.some(event => ['error', 'response.failed'].includes(event.type)));
    const terminal = evidence.events.find(event => event.type === 'response.completed');
    const text = terminal.response.output.filter(item => item.type === 'message')
      .flatMap(item => item.content || []).filter(part => part.type === 'output_text').map(part => part.text).join('');
    assert.equal(text, 'LEASE_COMPLETED', 'Attached terminal must contain the original continuation output');
    const outputItems = evidence.events.filter(event => event.type === 'response.output_item.done');
    assert.equal(outputItems.filter(event => event.item?.id === 'item_lease_active').length, 1,
      'Original provider output must be delivered exactly once');
    evidence.roundIdentityAfter = callbackRoundIdentity(sql);
    assertCallbackRoundIdentity(evidence.roundIdentityAfter);
    assert.deepEqual(evidence.roundIdentityAfter, evidence.roundIdentityBefore,
      'HTTP attachment must reuse the original WS round and generation');
    evidence.upstream = mock.trace;
    assert.equal(mock.trace.length, 2, 'Seed and exactly one continuation; retry must not invoke provider again');
    evidence.after = JSON.parse(sql("select jsonb_build_object('attempts',(select count(*) from flow_run_callback_resume_attempts),'claims',(select count(*) from flow_run_resume_claims),'completed_callbacks',(select count(*) from flow_run_callback_tasks where status='completed'));"));
    assert.equal(evidence.after.attempts, 1);
    assert.equal(evidence.after.claims, 1);
    assert.equal(evidence.after.completed_callbacks, 1);
    evidence.apiSha256 = crypto.createHash('sha256').update(fs.readFileSync(process.env.NRA_API_BINARY)).digest('hex');
    evidence.pass = true;
    save();
    console.log(JSON.stringify({ pass: true, httpStatus: retry.status, upstreamRequests: mock.trace.length, artifactRoot }));
  } catch (error) {
    evidence.failure = error.message;
    evidence.upstream = mock.trace;
    if (sql) {
      evidence.finalState = sql("select jsonb_build_object('run',r.id,'status',r.status,'error',r.error_payload,'attempt',a.status,'claim',c.status) from flow_runs r left join flow_run_callback_resume_attempts a on a.flow_run_id=r.id left join flow_run_resume_claims c on c.callback_task_id=a.callback_task_id;");
      evidence.roundIdentity = sql("select jsonb_build_object('sequence',sequence,'type',event_type,'round',payload->>'response_round_id','generation',payload->>'stream_generation_id','callback',payload->>'callback_task_id','stream_sequence',payload->'stream_sequence','sequence_end',payload->'sequence_end','resolved_round',runtime_event_original_payload(payload,raw_json_payloads,flow_run_id)->>'response_round_id','resolved_generation',runtime_event_original_payload(payload,raw_json_payloads,flow_run_id)->>'stream_generation_id') from runtime_events order by sequence;");
      evidence.callbackIdentity = sql("select jsonb_build_object('id',id,'status',status,'created_at',created_at,'completed_at',completed_at) from flow_run_callback_tasks order by created_at;");
      evidence.eventTail = sql("select jsonb_build_object('type',event_type,'sequence',sequence,'payload',left(payload::text,500)) from runtime_events order by created_at desc limit 12;");
    }
    save();
    throw error;
  } finally {
    clearTimeout(watchdog);
    clearTimeout(outputTimer);
    socket?.terminate();
    try { await gateway?.close(); } finally { await mock.close(); }
  }
}
if (require.main === module) run().catch(error => { console.error(error.stack); process.exitCode = 1; });
module.exports = { run };
