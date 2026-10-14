'use strict';
const crypto = require('node:crypto');

function syntheticBatch() {
  const run = crypto.randomUUID();
  const source_id = `exchange-demo-${run}`;
  const events = [];
  for (let task = 1; task <= 3; task++) {
    const kinds = ['context', 'user', 'tool_call', 'tool_result', 'assistant', 'usage', 'task_end'];
    for (const [index, kind] of kinds.entries()) {
      const content = {
        user: `Synthetic exchange fixture ${task}: what is ${task} plus one?`,
        tool_call: JSON.stringify({ value: task }), tool_result: String(task + 1),
        assistant: `Synthetic answer ${task + 1}.`, task_end: `Synthetic answer ${task + 1}.`
      }[kind] ?? null;
      const event_id = `${run}-task-${task}-${kind}`;
      events.push({
        event_id, source_session_id: `${run}-session-${task === 3 ? 2 : 1}`,
        source_task_id: `${run}-task-${task}`, parent_source_task_id: null,
        sequence: task * 10 + index, occurred_at: new Date(Date.now() + index).toISOString(),
        kind, content, phase: ['assistant', 'task_end'].includes(kind) ? 'final_answer' : null,
        name: kind === 'tool_call' ? 'fixture_increment' : null,
        call_id: ['tool_call', 'tool_result'].includes(kind) ? `${run}-call-${task}` : null,
        model_id: 'synthetic-fixture-model', reasoning_effort: 'max', provider_code: 'synthetic',
        usage: kind === 'usage' ? { basis: 'delta', response_id: `${run}-response-${task}`,
          input_tokens: 10 + task, output_tokens: 3, total_tokens: 13 + task,
          input_cache_hit_tokens: 0, cache_write_tokens: 0 } : null,
        inherited: false, raw: { synthetic_fixture: true, marker: event_id, context: { task, session_count: 2 }, original_content: content }
      });
    }
  }
  return { schema_version: '1flowbase.agent-logs/v1', source_id, source_client: 'synthetic-exchange-fixture', events };
}
module.exports = { syntheticBatch };
