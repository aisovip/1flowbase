import { beforeEach, expect, test, vi } from 'vitest';
import { apiFetch } from '../../../transport';
import {
  queryConsoleConversationClientTrajectory,
  getConsoleConversationClientTrajectorySection,
  type ConversationClientTrajectoryQueryPage
} from '../conversation-trajectory';
vi.mock('../../../transport', () => ({
  apiFetch: vi.fn().mockResolvedValue({})
}));
beforeEach(() => vi.mocked(apiFetch).mockClear());
test('canonical query preserves exact server filters, focus and both opaque directions', async () => {
  const input = {
    filter: {
      category: { $eq: 'tool_call' },
      is_subagent: { $eq: true },
      created_at: { $gte: '2026-10-12T00:00:00.123456Z' }
    },
    keyword: 'unloaded match',
    search_sections: ['parameters', 'result'],
    focus_record_id: 'main-round',
    limit: 1
  };
  for (const cursor of [
    undefined,
    'older/opaque+cursor',
    'newer/opaque+cursor'
  ]) {
    await queryConsoleConversationClientTrajectory(
      'app',
      'canonical',
      { ...input, cursor },
      'https://api.example'
    );
    expect(apiFetch).toHaveBeenLastCalledWith({
      path: '/api/console/applications/app/logs/conversations/canonical/client-trajectory/query',
      method: 'POST',
      body: { ...input, cursor },
      baseUrl: 'https://api.example'
    });
  }
});
test('canonical sections keep the original section DTO and fetch raw only on demand', async () => {
  await getConsoleConversationClientTrajectorySection(
    'app',
    'canonical',
    'step/locator',
    'raw',
    0,
    'https://api.example'
  );
  expect(apiFetch).toHaveBeenLastCalledWith({
    path: '/api/console/applications/app/logs/conversations/canonical/client-trajectory/step%2Flocator?limit=8&section=raw&cursor=0',
    baseUrl: 'https://api.example'
  });
});
test('query returns exact provenance and search fields without aliases', async () => {
  const page = {
    log_conversation_id: 'canonical',
    items: [
      {
        id: 'child-event',
        record_id: 'child',
        category: 'tool_call',
        is_subagent: true,
        parent_task_run_id: 'main',
        source_kind: 'imported',
        source_id: 'source',
        source_client: 'codex',
        source_session_id: 'session',
        source_task_id: 'task'
      }
    ],
    matches: [
      {
        step_id: 'child-event',
        section: 'result',
        sequence: 4,
        snippet: 'match'
      }
    ],
    previous_cursor: 'before',
    next_cursor: 'after',
    search_sections: ['result'],
    integrity: 'complete'
  };
  vi.mocked(apiFetch).mockResolvedValueOnce(
    page as unknown as ConversationClientTrajectoryQueryPage
  );
  expect(
    await queryConsoleConversationClientTrajectory('app', 'canonical')
  ).toBe(page);
});

test('exact index acquisition and selected unloaded event keep canonical membership', async () => {
  const query = {
    index_only: true,
    focus_record_id: 'initial-round',
    focus_step_id: 'unloaded-child-event'
  };
  await queryConsoleConversationClientTrajectory('app', 'canonical', query);
  expect(apiFetch).toHaveBeenLastCalledWith({
    path: '/api/console/applications/app/logs/conversations/canonical/client-trajectory/query',
    method: 'POST',
    body: query,
    baseUrl: undefined
  });
});
