'use strict';
// Acceptance readback uses the public API supported by the remote deployment.
const assert = require('node:assert/strict');

function cursor(page) {
  assert.ok(page.meta && typeof page.meta === 'object' && !Array.isArray(page.meta), 'Langfuse cursor metadata');
  assert.ok(!Object.keys(page.meta).length || Object.hasOwn(page.meta, 'cursor') || Object.hasOwn(page.meta, 'nextCursor'), 'Langfuse cursor field');
  const values = ['cursor', 'nextCursor'].map(key => page.meta[key]).filter(value => value != null);
  assert.ok(values.every(value => typeof value === 'string' && value.length), 'Langfuse nonempty cursor');
  assert.ok(values.length < 2 || values[0] === values[1], 'Langfuse conflicting cursors');
  return values[0];
}
function merge(items, rows, traceId) {
  assert.ok(Array.isArray(rows), 'Langfuse observations array');
  for (const row of rows) {
    assert.equal(row.traceId, traceId, 'remote observation trace binding');
    assert.ok(typeof row.id === 'string' && row.id, 'remote observation identity');
    const semantic = { ...row }; delete semantic.createdAt; delete semantic.updatedAt;
    const previous = items.get(row.id);
    if (previous) {
      const priorSemantic = { ...previous }; delete priorSemantic.createdAt; delete priorSemantic.updatedAt;
      assert.deepEqual(semantic, priorSemantic, 'remote duplicate observation semantic equality');
    } else items.set(row.id, row);
  }
}
async function readTraceObservations({ baseUrl, authorization, traceId, deadlineMs, fetchImpl = fetch }) {
  const items = new Map(), seen = new Set();
  let next, pageNumber = 1, legacy = false;
  for (;;) {
    const url = new URL(legacy ? 'api/public/observations' : 'api/public/v2/observations', baseUrl.endsWith('/') ? baseUrl : baseUrl + '/');
    url.searchParams.set('traceId', traceId);
    if (legacy) url.searchParams.set('page', String(pageNumber));
    else {
      url.searchParams.set('fields', 'basic,time,io,metadata,model,usage,trace_context');
      url.searchParams.set('expandMetadata', '1flowbase_agent_log');
      if (next) url.searchParams.set('cursor', next);
    }
    const response = await fetchImpl(url, { headers: { Authorization: authorization }, redirect: 'error', signal: AbortSignal.timeout(deadlineMs) });
    const page = await response.json();
    // A known older-deployment capability response permits the legacy reader.
    if (!legacy && !next && response.status === 404 && /observations v2 api is only available/i.test(page.message || '')) {
      legacy = true; continue;
    }
    assert.ok(response.ok, `Langfuse observations HTTP ${response.status}`);
    merge(items, page.data, traceId);
    if (legacy) {
      const total = page.meta?.totalPages;
      assert.ok(Number.isSafeInteger(total) && total >= 0 && page.meta.page === pageNumber, 'Langfuse numbered metadata');
      assert.ok(pageNumber <= total || (pageNumber === 1 && total === 0 && !page.data.length), 'Langfuse numbered page bounds');
      if (pageNumber >= total) return [...items.values()];
      assert.ok(page.data.length, 'Langfuse empty intermediate page');
      pageNumber++; continue;
    }
    next = cursor(page);
    if (!next) return [...items.values()];
    assert.ok(page.data.length && !seen.has(next), 'Langfuse cursor progress');
    seen.add(next);
  }
}
module.exports = { readTraceObservations };
