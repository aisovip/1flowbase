'use strict';
const http = require('node:http');
const { acceptWebSocket, createFrameReader, sendJson } = require('../mock-upstream/websocket');
const { responsesEvents } = require('../mock-upstream/protocol-events');

async function createActiveLeaseMock({ allowRecovery = false } = {}) {
  const trace = [];
  const sockets = new Set();
  let release;
  let ready;
  let recoveryReady;
  const recovering = new Promise(resolve => { recoveryReady = resolve; });
  const continuing = new Promise(resolve => { ready = resolve; });
  // Both transports spend the same ordinal and share the same controlled
  // completion. Switching to HTTP cannot obtain a second recovery allowance.
  function invoke(body, transport, { emit, end, reject }) {
    const ordinal = trace.length + 1;
    trace.push({ ordinal, transport, request: body, model: body.model, reasoning: body.reasoning,
      previous_response_id: body.previous_response_id });
    if ((transport === 'http' && (!allowRecovery || ordinal !== 3)) || ordinal > (allowRecovery ? 3 : 2)) {
      reject({ type: 'error', error: { code: 'duplicate_execution', message: 'Callback was executed twice' }, sequence_number: 0 });
      return;
    }
    let sequence = 0;
    const emitEvent = event => emit({ ...event, sequence_number: sequence++ });
    const responseId = `resp_lease_fixture_${ordinal}`;
    if (ordinal === 1) {
      const item = { type: 'function_call', id: 'fc_lease_fixture', call_id: 'call_lease_fixture',
        name: 'fixture', arguments: '{}' };
      emitEvent({ type: 'response.created', response: { id: responseId, status: 'in_progress', output: [] } });
      emitEvent({ type: 'response.output_item.added', output_index: 0, item: { ...item, arguments: '' } });
      emitEvent({ type: 'response.function_call_arguments.delta', output_index: 0, item_id: item.id, delta: '{}' });
      emitEvent({ type: 'response.function_call_arguments.done', output_index: 0, item_id: item.id, arguments: '{}' });
      emitEvent({ type: 'response.output_item.done', output_index: 0, item: { ...item, status: 'completed' } });
      emitEvent({ type: 'response.completed', response: { id: responseId, status: 'completed', output: [item],
        usage: { input_tokens: 10, output_tokens: 10, total_tokens: 20 } } });
      end();
    } else {
      const response = responsesEvents('lease_active', 'LEASE_', 'COMPLETED');
      response.chunks[0].response.model = body.model;
      response.terminal.response.model = body.model;
      emitEvent(response.chunks[0]);
      release = () => {
        trace[ordinal - 1].released = true;
        for (const event of response.chunks.slice(1)) emitEvent(event);
        emitEvent(response.terminal);
        end();
      };
      if (ordinal === 2) ready();
      else recoveryReady();
    }
  }
  const server = http.createServer(async (req, res) => {
    if (req.url === '/v1/models') {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ data: [{ id: 'gpt-6-luna', object: 'model' }] }));
    } else if (allowRecovery && req.method === 'POST' && req.url === '/v1/responses') {
      try {
        const chunks = [];
        for await (const chunk of req) chunks.push(chunk);
        const body = JSON.parse(Buffer.concat(chunks).toString());
        invoke(body, 'http', {
          emit(event) {
            if (!res.headersSent) res.writeHead(200, { 'content-type': 'text/event-stream' });
            res.write(`data: ${JSON.stringify(event)}\n\n`);
          },
          end() { res.end('data: [DONE]\n\n'); },
          reject(event) { res.writeHead(409, { 'content-type': 'application/json' }); res.end(JSON.stringify(event)); },
        });
      } catch (error) {
        if (!res.headersSent) res.writeHead(400, { 'content-type': 'text/plain' });
        res.end(error.message);
      }
    } else res.writeHead(503).end('Unexpected new upstream request');
  });
  server.on('connection', socket => {
    sockets.add(socket);
    socket.once('close', () => sockets.delete(socket));
  });
  server.on('upgrade', (req, socket, head) => {
    if (!acceptWebSocket(req, socket)) return;
    createFrameReader(socket, raw => {
      const emit = event => sendJson(socket, event);
      invoke(JSON.parse(raw), 'websocket', { emit, end() {}, reject: emit });
    }, () => {});
    if (head.length) socket.emit('data', head);
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  return { baseUrl: `http://127.0.0.1:${server.address().port}`, trace, continuing, recovering,
    release() { if (!release) throw new Error('Continuation has not started'); release(); },
    async close() { for (const socket of sockets) socket.destroy(); await new Promise(resolve => server.close(resolve)); } };
}
module.exports = { createActiveLeaseMock };
