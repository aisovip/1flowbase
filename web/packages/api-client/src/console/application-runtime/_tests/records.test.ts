import { beforeEach, expect, test, vi } from 'vitest';
import { apiFetch } from '../../../transport';
import {
  getConsoleApplicationLogRecordTraceTree,
  getConsoleApplicationLogRecordTraceChildren,
  getConsoleApplicationLogRecordConversationMessages,
  queryConsoleApplicationLogConversations,
  getConsoleApplicationLogRecord,
  getConsoleApplicationLogRecordClientTrajectory,
  getConsoleApplicationLogRecordClientTrajectorySection
} from '../records';
import { getConsoleClientTrajectory } from '../client-trajectory';
vi.mock('../../../transport', () => ({
  apiFetch: vi.fn().mockResolvedValue({})
}));
beforeEach(() => vi.mocked(apiFetch).mockClear());
test('record overview uses its source-neutral record route', async () => {
  await getConsoleApplicationLogRecord('app', 'record', 'https://api.example');
  expect(apiFetch).toHaveBeenLastCalledWith({
    path: '/api/console/applications/app/logs/records/record',
    baseUrl: 'https://api.example'
  });
});
test('record trajectory preserves exact focus and cursors and sections stay scoped to record', async () => {
  const options = {
    request_id: 'source-session',
    focus_step_id: 'source-event'
  };
  await getConsoleApplicationLogRecordClientTrajectory(
    'app',
    'record',
    undefined,
    options
  );
  expect(apiFetch).toHaveBeenLastCalledWith({
    path: '/api/console/applications/app/logs/records/record/client-trajectory?limit=50&request_id=source-session&focus_step_id=source-event',
    baseUrl: undefined
  });
  await getConsoleApplicationLogRecordClientTrajectory(
    'app',
    'record',
    99,
    options
  );
  expect(apiFetch).toHaveBeenLastCalledWith({
    path: '/api/console/applications/app/logs/records/record/client-trajectory?limit=100&cursor=99&request_id=source-session',
    baseUrl: undefined
  });
  await getConsoleApplicationLogRecordClientTrajectorySection(
    'app',
    'record',
    'step',
    'raw',
    88
  );
  expect(apiFetch).toHaveBeenLastCalledWith({
    path: '/api/console/applications/app/logs/records/record/client-trajectory/step?limit=8&section=raw&cursor=88',
    baseUrl: undefined
  });
  await getConsoleClientTrajectory('app', 'native-run');
  expect(apiFetch).toHaveBeenLastCalledWith({
    path: '/api/console/applications/app/logs/runs/native-run/client-trajectory?limit=50',
    baseUrl: undefined
  });
});

test('record API passes opaque same-sequence paging tokens unchanged while native and body cursors remain numeric', async () => {
  const cursor = 's1:10:018f0000-0000-7000-8000-000000000001';
  await getConsoleApplicationLogRecordClientTrajectory('app', 'record', cursor);
  const request = vi.mocked(apiFetch).mock.calls.at(-1)![0];
  const url = new URL(request.path, 'https://example.test');
  expect(url.pathname).toBe(
    '/api/console/applications/app/logs/records/record/client-trajectory'
  );
  expect(url.searchParams.get('cursor')).toBe(cursor);
  expect(url.searchParams.get('limit')).toBe('100');
  await getConsoleClientTrajectory('app', 'native-run', undefined, 10);
  expect(apiFetch).toHaveBeenLastCalledWith({
    path: '/api/console/applications/app/logs/runs/native-run/client-trajectory?limit=100&cursor=10',
    baseUrl: undefined
  });
  await getConsoleApplicationLogRecordClientTrajectorySection(
    'app',
    'record',
    'step',
    'raw',
    10
  );
  expect(apiFetch).toHaveBeenLastCalledWith({
    path: '/api/console/applications/app/logs/records/record/client-trajectory/step?limit=8&section=raw&cursor=10',
    baseUrl: undefined
  });
});

test('imported trace metadata and nested children use record routes with bound opaque cursors', async () => {
  await getConsoleApplicationLogRecordTraceTree(
    'app',
    'main-record',
    'https://api.example'
  );
  expect(apiFetch).toHaveBeenLastCalledWith({
    path: '/api/console/applications/app/logs/records/main-record/trace-tree',
    baseUrl: 'https://api.example'
  });
  await getConsoleApplicationLogRecordTraceChildren(
    'app',
    'main-record',
    'child-record',
    'opaque:child+page',
    20
  );
  const request = vi.mocked(apiFetch).mock.calls.at(-1)![0];
  const url = new URL(request.path, 'https://example.test');
  expect(url.pathname).toBe(
    '/api/console/applications/app/logs/records/main-record/trace-tree/child-record/children'
  );
  expect(url.searchParams.get('cursor')).toBe('opaque:child+page');
  expect(url.searchParams.get('page_size')).toBe('20');
});
test('main transcript paging has no child scope and conversations query retains canonical page fields', async () => {
  await getConsoleApplicationLogRecordConversationMessages(
    'app',
    'child-anchor',
    { before: 'main-turn-cursor', limit: 5 }
  );
  expect(apiFetch).toHaveBeenLastCalledWith({
    path: '/api/console/applications/app/logs/records/child-anchor/conversation/messages?before=main-turn-cursor&limit=5',
    baseUrl: undefined
  });
  const response = {
    items: [
      {
        record_id: 'latest-main',
        log_conversation_id: 'canonical',
        total_tokens: 123
      }
    ],
    total: 8,
    page: 2,
    page_size: 5
  };
  vi.mocked(apiFetch).mockResolvedValueOnce(response);
  const input = {
    application_ids: ['app'],
    filter: { title: { $includes: 'main' } },
    keyword: 'question',
    sort_field: 'total_tokens',
    descending: false,
    page: 2,
    page_size: 5
  };
  expect(
    await queryConsoleApplicationLogConversations(input, 'https://api.example')
  ).toBe(response);
  expect(apiFetch).toHaveBeenLastCalledWith({
    path: '/api/console/applications/logs/conversations/query',
    method: 'POST',
    body: input,
    baseUrl: 'https://api.example'
  });
});
