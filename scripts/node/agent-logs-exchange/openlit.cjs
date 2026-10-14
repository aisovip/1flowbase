'use strict';
// Child receives secrets only over stdin; the parent saves SDK diagnostics to a private mode-0600 log.
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const { createRequire } = require('node:module');

async function main() {
  let input = '';
  for await (const chunk of process.stdin) input += chunk;
  const config = JSON.parse(input);
  const requireSdk = createRequire(path.join(config.moduleRoot, '__agent_logs_fixture.cjs'));
  const packageFile = path.join(config.moduleRoot, 'openlit/package.json');
  assert.equal(JSON.parse(fs.readFileSync(packageFile, 'utf8')).version, '1.15.0');
  const imported = requireSdk('openlit');
  const openlit = imported.default ?? imported.Openlit;
  const { trace, SpanStatusCode } = requireSdk('@opentelemetry/api');
  openlit.init({ otlpEndpoint: config.apiBaseUrl + '/api/logs/otlp',
    otlpHeaders: { Authorization: 'Bearer ' + config.key },
    disableMetrics: true, disableEvents: true, disableBatch: true,
    applicationName: 'Agent Logs OpenLIT real SDK fixture' });
  const proxy = trace.getTracerProvider();
  const provider = typeof proxy.getDelegate === 'function' ? proxy.getDelegate() : proxy;
  assert.equal(typeof provider.forceFlush, 'function', 'OpenLIT must register a real SDK provider');
  const tracer = trace.getTracer('agent-logs-openlit-fixture', '1.15.0');
  const spans = [];
  try {
    for (const [index, message] of config.messages.entries()) {
      const span = tracer.startSpan('gen_ai.chat');
      assert.ok(span.isRecording(), 'tracer must record');
      span.setAttributes({
        'gen_ai.system': 'openai', 'gen_ai.operation.name': 'chat',
        'gen_ai.request.model': message.model, 'gen_ai.response.model': message.model,
        'gen_ai.request.reasoning_effort': 'max',
        'session.id': config.sessionIds[index], 'gen_ai.conversation.id': config.sessionIds[index],
        'gen_ai.input.messages': JSON.stringify([{ role: 'user', content: message.prompt }]),
        'gen_ai.output.messages': JSON.stringify([{ role: 'assistant', content: message.answer }]),
        'gen_ai.prompt': message.prompt, 'gen_ai.completion': message.answer,
        'gen_ai.usage.input_tokens': message.input_tokens, 'gen_ai.usage.output_tokens': message.output_tokens,
        'gen_ai.usage.total_tokens': message.input_tokens + message.output_tokens,
        'fixture.synthetic': true, 'fixture.message_source': message.source
      });
      span.setStatus({ code: SpanStatusCode.OK });
      spans.push({ ...span.spanContext(), source_session_id: config.sessionIds[index] });
      span.end();
    }
    await provider.forceFlush();
    fs.writeFileSync(config.summaryPath, JSON.stringify({ sdk: 'openlit', version: '1.15.0', spans }, null, 2), { mode: 0o600 });
  } finally { await provider.shutdown(); }
}
main().catch(() => { process.exitCode = 1; });
