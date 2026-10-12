'use strict';
const http = require('node:http');
const { acceptWebSocket, createFrameReader, sendJson } = require('../mock-upstream/websocket');

async function createMock() {
  const requests = [];
  const sockets = new Set();
  const server = http.createServer((req, res) => {
    if (req.url.includes('/models')) {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ data: [{ id: 'gpt-6-luna', object: 'model' }] }));
      return;
    }
    res.writeHead(503, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ error: { message: 'Unexpected HTTP fallback in target handoff fixture' } }));
  });
  server.on('upgrade', (req, socket, head) => {
    if (!acceptWebSocket(req, socket)) return;
    sockets.add(socket);
    socket.on('close', () => sockets.delete(socket));
    createFrameReader(socket, raw => {
      const request = JSON.parse(raw);
      requests.push({ model: request.model, reasoning: request.reasoning,
        previous_response_id: request.previous_response_id ?? null });
      const ordinal = requests.length;
      let sequence = 0;
      const emit = event => sendJson(socket, { ...event, sequence_number: sequence++ });
      const id = `resp_handoff_${ordinal}`;
      const text = ordinal === 2 ? 'Summary retains HANDOFF_SENTINEL.' : 'HANDOFF_SENTINEL';
      const item = { type: 'message', id: `msg_handoff_${ordinal}`, role: 'assistant',
        status: 'completed', content: [{ type: 'output_text', text, annotations: [] }] };
      emit({ type: 'response.created', response: { id, status: 'in_progress' } });
      emit({ type: 'response.output_item.added', output_index: 0, item: { ...item, status: 'in_progress', content: [] } });
      emit({ type: 'response.output_text.delta', output_index: 0, content_index: 0, item_id: item.id, delta: text });
      emit({ type: 'response.output_text.done', output_index: 0, content_index: 0, item_id: item.id, text });
      emit({ type: 'response.output_item.done', output_index: 0, item });
      emit({ type: 'response.completed', response: { id, status: 'completed', output: [item],
        usage: { input_tokens: 20, output_tokens: 10, total_tokens: 30 } } });
    });
    if (head.length) socket.emit('data', head);
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  return { baseUrl: `http://127.0.0.1:${server.address().port}`, requests,
    async close() { for (const socket of sockets) socket.destroy(); await new Promise(resolve => server.close(resolve)); } };
}
module.exports = { createMock };
