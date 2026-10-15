'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readTraceObservations } = require('../remote-readback.cjs');
const row = (id, extra={}) => ({id, traceId:'trace', startTime:'2026-10-15T00:00:00Z', metadata:{fixture:'exact'}, ...extra});
const options = {baseUrl:'https://langfuse.example/prefix/', authorization:'fixture-only', traceId:'trace', deadlineMs:1000};
function responses(pages, seen=[]) {
  return async url => {
    seen.push(new URL(url)); assert.ok(pages.length, 'unexpected extra remote call');
    const {status=200, body} = pages.shift();
    return new Response(JSON.stringify(body), {status, headers:{'content-type':'application/json'}});
  };
}
test('readback drains cursor pages, binds trace and collapses bookkeeping versions', async () => {
  const seen=[];
  const result = await readTraceObservations({...options, fetchImpl:responses([
    {body:{data:[row('one',{createdAt:'old',updatedAt:'old'})],meta:{cursor:'next'}}},
    {body:{data:[row('one',{createdAt:'new',updatedAt:'new'}),row('two')],meta:{nextCursor:null}}},
  ],seen)});
  assert.deepEqual(result.map(value=>value.id),['one','two']);
  assert.ok(seen.every(url=>url.pathname==='/prefix/api/public/v2/observations' && url.searchParams.get('traceId')==='trace'));
  assert.equal(seen[1].searchParams.get('cursor'),'next');
});
test('only explicit older-v2 capability permits numbered observations fallback', async () => {
  const seen=[];
  const result = await readTraceObservations({...options, fetchImpl:responses([
    {status:404,body:{message:'The observations v2 API is only available in a Langfuse v4 write mode.'}},
    {body:{data:[row('one')],meta:{page:1,totalPages:2}}},
    {body:{data:[row('two')],meta:{page:2,totalPages:2}}},
  ],seen)});
  assert.equal(result.length,2);
  assert.equal(seen[2].pathname,'/prefix/api/public/observations');
  assert.equal(seen[2].searchParams.get('page'),'2');
  await assert.rejects(readTraceObservations({...options,fetchImpl:responses([{status:404,body:{message:'Project not found'}}])}), /HTTP 404/);
});
test('readback rejects foreign identities, semantic conflicts and nonprogressing cursors', async () => {
  for(const pages of [
    [{body:{data:[row('one',{traceId:'foreign'})],meta:{cursor:null}}}],
    [{body:{data:[row('one'),row('one',{metadata:{fixture:'changed'}})],meta:{cursor:null}}}],
    [{body:{data:[row('one')],meta:{cursor:'loop'}}},{body:{data:[row('two')],meta:{cursor:'loop'}}}],
    [{body:{data:[],meta:{cursor:'not-complete'}}}],
    [{body:{data:[row('one')],meta:{cursor:'a',nextCursor:'b'}}}],
  ]) await assert.rejects(readTraceObservations({...options,fetchImpl:responses(pages)}));
});
