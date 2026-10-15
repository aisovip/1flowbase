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
  const server = http.createServer((req, res) => {
    if (req.url === '/v1/models') {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ data: [{ id: 'gpt-6-luna', object: 'model' }] }));
    } else res.writeHead(503).end('Unexpected new upstream request');
  });
  server.on('connection', socket => {
    sockets.add(socket);
    socket.once('close', () => sockets.delete(socket));
  });
  server.on('upgrade', (req, socket, head) => {
    if (!acceptWebSocket(req, socket)) return;
    createFrameReader(socket, raw => {
      const body = JSON.parse(raw);
      const ordinal = trace.length + 1;
      trace.push({ ordinal, model: body.model, reasoning: body.reasoning,
        previous_response_id: body.previous_response_id });
      let sequence = 0;
      const emit = event => sendJson(socket, { ...event, sequence_number: sequence++ });
      const responseId = `resp_lease_fixture_${ordinal}`;
      if (ordinal === 1) {
        const item = { type: 'function_call', id: 'fc_lease_fixture', call_id: 'call_lease_fixture',
          name: 'fixture', arguments: '{}' };
        emit({ type: 'response.created', response: { id: responseId, status: 'in_progress', output: [] } });
        emit({ type: 'response.output_item.added', output_index: 0, item: { ...item, arguments: '' } });
        emit({ type: 'response.function_call_arguments.delta', output_index: 0, item_id: item.id, delta: '{}' });
        emit({ type: 'response.function_call_arguments.done', output_index: 0, item_id: item.id, arguments: '{}' });
        emit({ type: 'response.output_item.done', output_index: 0, item: { ...item, status: 'completed' } });
        emit({ type: 'response.completed', response: { id: responseId, status: 'completed', output: [item],
          usage: { input_tokens: 10, output_tokens: 10, total_tokens: 20 } } });
      } else if (ordinal === 2 || (allowRecovery && ordinal === 3)) {
        const response = responsesEvents('lease_active', 'LEASE_', 'COMPLETED');
        response.chunks[0].response.model = body.model;
        response.terminal.response.model = body.model;
        emit(response.chunks[0]);
        release = () => {
          trace[ordinal - 1].released = true;
          for (const event of response.chunks.slice(1)) emit(event);
          emit(response.terminal);
        };
        if (ordinal === 2) ready();
        else recoveryReady();
      } else {
        emit({ type: 'error', error: { code: 'duplicate_execution', message: 'Callback was executed twice' } });
      }
    }, () => {});
    if (head.length) socket.emit('data', head);
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  return { baseUrl: `http://127.0.0.1:${server.address().port}`, trace, continuing, recovering,
    release() { if (!release) throw new Error('Continuation has not started'); release(); },
    async close() { for (const socket of sockets) socket.destroy(); await new Promise(resolve => server.close(resolve)); } };
}
module.exports = { createActiveLeaseMock };
