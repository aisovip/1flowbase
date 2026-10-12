import { beforeEach, expect, test, vi } from 'vitest';
import { apiFetch } from '../../../transport';
import {
  getConsoleWorkflowTrajectory,
  getConsoleWorkflowTrajectoryBody,
  getConsoleProviderTrajectory,
  getConsoleRunTrajectory
} from '../trajectory';
import { getConsoleClientTrajectory } from '../client-trajectory';
vi.mock('../../../transport', () => ({
  apiFetch: vi.fn().mockResolvedValue({})
}));
beforeEach(() => {
  vi.mocked(apiFetch).mockClear();
});

test('native focus and trigger filter remain scoped and focus is only sent on the first page', async () => {
  const options = { request_id: 'request-B', focus_event_id: 'event-late' };
  await getConsoleProviderTrajectory(
    'app',
    'run',
    'node',
    undefined,
    undefined,
    options
  );
  expect(apiFetch).toHaveBeenLastCalledWith({
    path: '/api/console/applications/app/logs/runs/run/nodes/node/trajectory?limit=50&request_id=request-B&focus_event_id=event-late',
    baseUrl: undefined
  });
  await getConsoleRunTrajectory('app', 'run', 950, undefined, options);
  expect(apiFetch).toHaveBeenLastCalledWith({
    path: '/api/console/applications/app/logs/runs/run/trajectory?limit=100&request_id=request-B&cursor=950',
    baseUrl: undefined
  });
});
test('client focus uses the supplied request identity without fetching preceding pages', async () => {
  const options = { request_id: 'request-A', focus_step_id: 'request-A' };
  await getConsoleClientTrajectory(
    'app',
    'old-run',
    undefined,
    undefined,
    undefined,
    options
  );
  expect(apiFetch).toHaveBeenLastCalledWith({
    path: '/api/console/applications/app/logs/runs/old-run/client-trajectory?limit=50&request_id=request-A&focus_step_id=request-A',
    baseUrl: undefined
  });
  await getConsoleClientTrajectory(
    'app',
    'old-run',
    undefined,
    42,
    undefined,
    options
  );
  expect(apiFetch).toHaveBeenLastCalledWith({
    path: '/api/console/applications/app/logs/runs/old-run/client-trajectory?limit=100&request_id=request-A&cursor=42',
    baseUrl: undefined
  });
  expect(apiFetch).toHaveBeenCalledTimes(2);
});

test('workflow summary sends exact server filters and opaque cursors; body escapes event locators', async () => {
  await getConsoleWorkflowTrajectory('app', 'root', 'opaque/+cursor', {
    category: 'agents',
    node_run_id: 'node-execution',
    request_id: 'request-exact',
    from: '2026-09-22T00:00:00Z',
    to: '2026-09-22T01:00:00Z'
  });
  const call = vi.mocked(apiFetch).mock.calls[0][0];
  const url = new URL(call.path, 'https://example.test');
  expect(url.pathname).toBe(
    '/api/console/applications/app/logs/runs/root/workflow-trajectory'
  );
  expect(Object.fromEntries(url.searchParams)).toEqual({
    limit: '100',
    category: 'agents',
    node_run_id: 'node-execution',
    request_id: 'request-exact',
    from: '2026-09-22T00:00:00Z',
    to: '2026-09-22T01:00:00Z',
    cursor: 'opaque/+cursor'
  });
  await getConsoleWorkflowTrajectoryBody('app', 'root', 'workflow:a/b');
  expect(apiFetch).toHaveBeenLastCalledWith({
    path: '/api/console/applications/app/logs/runs/root/workflow-trajectory/workflow%3Aa%2Fb',
    baseUrl: undefined
  });
});

test('native index and visible focus windows preserve node/request scope and exact acquisition controls', async () => {
  await getConsoleClientTrajectory('app', 'run', 'node', undefined, undefined, {
    request_id: 'request',
    index_only: true,
    category: 'tool_call',
    keyword: 'unloaded NUL\0match',
    from: '2026-10-12T00:00:00Z'
  });
  const call = vi.mocked(apiFetch).mock.calls[0][0];
  const query = new URL(call.path, 'https://example.test').searchParams;
  expect(query.get('node_run_id')).toBe('node');
  expect(query.get('request_id')).toBe('request');
  expect(query.get('index_only')).toBe('true');
  expect(query.get('keyword')).toBe('unloaded NUL\0match');
  await getConsoleClientTrajectory('app', 'run', 'node', -105, undefined, {
    request_id: 'request'
  });
  expect(
    new URL(
      vi.mocked(apiFetch).mock.calls[1][0].path,
      'https://example.test'
    ).searchParams.get('cursor')
  ).toBe('-105');
  await getConsoleWorkflowTrajectory('app', 'run', undefined, {
    index_only: true,
    focus_event_id: 'native:unloaded'
  });
  const workflow = new URL(
    vi.mocked(apiFetch).mock.calls[2][0].path,
    'https://example.test'
  ).searchParams;
  expect(workflow.get('index_only')).toBe('true');
  expect(workflow.get('focus_event_id')).toBe('native:unloaded');
});
