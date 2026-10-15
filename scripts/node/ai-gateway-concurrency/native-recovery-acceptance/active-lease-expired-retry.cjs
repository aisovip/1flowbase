'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { decodeSseChunks } = require('../protocol-oracle/decoder');

const terminalTypes = new Set(['response.completed', 'response.failed', 'error']);
async function captureTerminal(response, artifactRoot, name) {
  const chunks = [];
  let events = [];
  const reader = response.body.getReader();
  try {
    for (;;) {
      const { value, done } = await reader.read();
      if (done) break;
      chunks.push(Buffer.from(value));
      events = decodeSseChunks(chunks).filter(record => !record.done).map(record => record.data);
      if (events.some(event => terminalTypes.has(event.type))) break;
    }
  } finally { await reader.cancel().catch(() => {}); }
  const raw = Buffer.concat(chunks).toString();
  fs.writeFileSync(path.join(artifactRoot, `${name}.sse`), raw);
  return { httpStatus: response.status, raw, events };
}

function originalCallbackReceipt(sql) {
  return JSON.parse(sql(`select jsonb_build_object(
    'run_id', a.flow_run_id,
    'callback', jsonb_build_object('id',t.id,'status',t.status,'response_payload',
      runtime_original_json(t.response_payload,t.raw_json_payloads,'response_payload'),'completed_at',t.completed_at),
    'inbox', (select jsonb_agg(jsonb_build_object('id',i.id,'tool_call_id',i.tool_call_id,
      'status',i.status,'result_payload',runtime_original_json(i.result_payload,i.raw_json_payloads,'result_payload'),
      'result_fingerprint',i.result_fingerprint,'received_at',i.received_at) order by i.tool_ordinal)
      from flow_run_tool_callback_inbox i where i.callback_task_id=t.id))
    from flow_run_callback_resume_attempts a join flow_run_callback_tasks t on t.id=a.callback_task_id;`));
}

function assertFullUpstreamRequest(mock, fullRequest) {
  assert.equal(mock.trace.length, 3, 'Only seed, abandoned continuation, and one full-context recovery may reach upstream');
  const request = mock.trace[2].request;
  assert.ok(!Object.hasOwn(request, 'previous_response_id'), 'Recovery upstream request must omit the unavailable continuation cursor');
  assert.deepEqual(request.input, fullRequest.input, 'Upstream must receive the real ordered full input, including the completed seed output');
  const outputs = request.input.filter(item => item.type === 'function_call_output');
  assert.equal(outputs.length, 1, 'The accepted tool output must occur exactly once in full-context inference');
  assert.deepEqual(outputs[0], fullRequest.input.at(-1));
}

async function beginExpiredFullContextRetry({ url, headers, followup, body, seedCompleted,
  mock, sql, evidence, save, artifactRoot, bounded }) {
  evidence.beforeIncrementalRetry = originalCallbackReceipt(sql);
  save();
  const incremental = await fetch(url, { method: 'POST', headers, body: JSON.stringify(followup),
    signal: AbortSignal.timeout(45_000) });
  evidence.incrementalRetry = await captureTerminal(incremental, artifactRoot, 'expired-incremental-retry');
  evidence.upstream = mock.trace;
  save();
  assert.equal(evidence.incrementalRetry.httpStatus, 200);
  const terminals = evidence.incrementalRetry.events.filter(event => terminalTypes.has(event.type));
  assert.equal(terminals.length, 1, 'The first recovery must have one recognizable retryable terminal');
  assert.equal(terminals[0].type, 'response.failed');
  assert.equal(terminals[0].response.status, 'failed');
  assert.equal(terminals[0].response.error.type, 'server_error');
  assert.equal(terminals[0].response.error.code, 'provider_transport_unavailable');
  assert.equal(mock.trace.length, 2, 'Unavailable continuation must fail before a third provider invocation');
  evidence.beforeFullContextRetry = originalCallbackReceipt(sql);
  assert.deepEqual(evidence.beforeFullContextRetry, evidence.beforeIncrementalRetry,
    'Incremental recovery must retain the already accepted callback and inbox receipt');
  assert.equal(evidence.beforeFullContextRetry.callback.status, 'completed');
  assert.equal(evidence.beforeFullContextRetry.inbox.length, 1);
  assert.equal(evidence.beforeFullContextRetry.inbox[0].status, 'received');
  assert.ok(Array.isArray(seedCompleted.response.output) && seedCompleted.response.output.length,
    'Full-context retry requires the real completed seed output');
  const fullRequest = { ...body, input: structuredClone([
    ...body.input, ...seedCompleted.response.output, ...followup.input,
  ]) };
  assert.ok(!Object.hasOwn(fullRequest, 'previous_response_id'));
  evidence.fullContextRetry = {
    request: fullRequest, seedCompletedOutput: seedCompleted.response.output,
    clientReference: {
      repository: 'https://github.com/openai/codex',
      commit: '7498521d288b9b3b96ffba4eedf089d8d6e06a84',
      evidence: 'Source inspection: unknown response.failed codes are Retryable; response cursor advances only after response.completed.',
    },
  };
  save();
  const retry = fetch(url, { method: 'POST', headers, body: JSON.stringify(fullRequest),
    signal: AbortSignal.timeout(45_000) });
  retry.catch(() => {});
  try {
    await bounded(mock.recovering, 'Full-context recovery did not reach its single new upstream invocation');
  } catch (error) {
    const response = await bounded(retry, 'Full-context retry did not return headers');
    evidence.fullContextRetry.result = await captureTerminal(response, artifactRoot, 'expired-full-context-failure');
    evidence.upstream = mock.trace;
    save();
    throw error;
  }
  assertFullUpstreamRequest(mock, fullRequest);
  evidence.during = originalCallbackReceipt(sql);
  assert.deepEqual(evidence.during, evidence.beforeFullContextRetry,
    'Full-context inference must not consume the original callback or inbox receipt again');
  mock.release();
  evidence.providerReleasedUtc = new Date().toISOString();
  save();
  return retry;
}

function assertExpiredFullContextResult({ mock, sql, evidence, terminal }) {
  assertFullUpstreamRequest(mock, evidence.fullContextRetry.request);
  evidence.afterFullContextRetry = originalCallbackReceipt(sql);
  assert.deepEqual(evidence.afterFullContextRetry, evidence.beforeFullContextRetry,
    'The original callback and accepted tool receipt must remain unchanged after full-context recovery');
  evidence.recoveryRuns = JSON.parse(sql(`select coalesce(jsonb_agg(jsonb_build_object('id',id,'status',status)
    order by started_at,id),'[]'::jsonb) from flow_runs
    where id not in (select flow_run_id from flow_run_callback_resume_attempts);`));
  assert.equal(evidence.recoveryRuns.length, 1, 'Verified full history must create exactly one new inference run');
  assert.notEqual(evidence.recoveryRuns[0].id, evidence.beforeFullContextRetry.run_id);
  assert.equal(evidence.recoveryRuns[0].status, 'succeeded');
  evidence.fullContextRetry.completedResponseId = terminal.response.id;
  evidence.fullContextRetry.recoveryRunId = evidence.recoveryRuns[0].id;
  assert.equal(mock.trace[1].released, undefined, 'Abandoned original execution must never produce output');
  assert.equal(mock.trace[2].released, true, 'Only the full-context inference may complete upstream');
}
module.exports = { beginExpiredFullContextRetry, assertExpiredFullContextResult };
