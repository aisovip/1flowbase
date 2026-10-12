import '../../../../test/fixtures/rc-util-unique-ids';
import type { ConsoleApplicationLogRecordOverview } from '@1flowbase/api-client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { App } from 'antd';
import { beforeEach, expect, test, vi } from 'vitest';
import { appI18n } from '../../../../shared/i18n/app-i18n';
import { ApplicationRunDetailPanel } from '../../components/logs/ApplicationRunDetailPanel';

const api = vi.hoisted(() => ({
  fetchApplicationLogRecordConversationMessages: vi.fn(),
  fetchApplicationLogRecordClientTrajectory: vi.fn(),
  fetchApplicationLogRecordClientTrajectorySection: vi.fn()
}));
vi.mock('../../api/trajectory', async (original) => ({
  ...(await original<typeof import('../../api/trajectory')>()),
  ...api
}));

function record(id: string, overrides: Partial<ConsoleApplicationLogRecordOverview> = {}): ConsoleApplicationLogRecordOverview {
  return {
    record_id: id,
    parent_task_run_id: null,
    child_task_count: 0,
    status: 'succeeded',
    source_id: 'source-1',
    source_kind: 'imported',
    source_client: 'codex',
    source_session_id: 'session-1',
    source_task_id: id,
    native_run_id: null,
    title: 'Task ' + id,
    outcome: 'final_answer_observed',
    started_at: '2026-10-12T00:00:00Z',
    finished_at: null,
    requested_model_id: null,
    reasoning_effort: null,
    messages: [
      { role: 'user', content: 'Question ' + id, sequence: 1 },
      { role: 'assistant', content: 'Answer ' + id, sequence: 2 }
    ],
    total_tokens: 0,
    input_tokens: 0,
    output_tokens: 0,
    input_cache_hit_tokens: 0,
    cost_breakdown: { total_cost: '0' },
    available_views: ['conversation', 'client_trajectory'],
    ...overrides
  };
}
function page(items: ConsoleApplicationLogRecordOverview[], overrides = {}) {
  return {
    items,
    page: {
      has_before: false, has_after: false, before_cursor: null,
      after_cursor: null, newest_cursor: 'latest', ...overrides
    }
  };
}
function mount(onOpenMessageLog = vi.fn()) {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <App>
        <ApplicationRunDetailPanel applicationId="app-1" runId="parent-2" recordId="parent-2"
          onClose={vi.fn()} onOpenMessageLog={onOpenMessageLog} />
      </App>
    </QueryClientProvider>
  );
}
function childArea(container: HTMLElement, parent = 'parent-1') {
  return container.querySelector('[data-parent-record-id="' + parent + '"]') as HTMLElement;
}
beforeEach(async () => {
  vi.clearAllMocks();
  await appI18n.changeLanguage('en_US');
  api.fetchApplicationLogRecordClientTrajectory.mockResolvedValue({
    items: [], next_cursor: null, integrity: 'complete'
  });
});

test('child area stays at its parent turn, lazily loads its transcript, and opens its own trajectory and details', async () => {
  api.fetchApplicationLogRecordConversationMessages.mockImplementation(async (_app, id, input) =>
    input.scope === 'children' ? page([record('child-1', { parent_task_run_id: 'parent-1' })])
      : page([record('parent-1', { child_task_count: 1 }), record('parent-2')])
  );
  const open = vi.fn();
  const view = mount(open);
  const answer1 = await screen.findByText('Answer parent-1');
  const answer2 = screen.getByText('Answer parent-2');
  const area = childArea(view.container);
  expect(answer1.compareDocumentPosition(area) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(area.compareDocumentPosition(answer2) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(api.fetchApplicationLogRecordConversationMessages).toHaveBeenCalledTimes(1);
  expect(screen.queryByText('Answer child-1')).not.toBeInTheDocument();
  fireEvent.click(within(area).getByRole('button', { name: 'Subagents (1)' }));
  expect(await screen.findByText('Answer child-1')).toBeInTheDocument();
  expect(api.fetchApplicationLogRecordConversationMessages).toHaveBeenCalledWith(
    'app-1', 'parent-1', { scope: 'children', limit: 5 }
  );
  expect(api.fetchApplicationLogRecordClientTrajectory).not.toHaveBeenCalled();
  fireEvent.click(within(area).getByRole('button', { name: 'Run trajectory' }));
  await waitFor(() => expect(api.fetchApplicationLogRecordClientTrajectory).toHaveBeenCalledWith(
    'app-1', 'child-1', undefined, undefined
  ));
  fireEvent.click(within(area).getByRole('button', { name: 'View conversation log' }));
  expect(open).toHaveBeenCalledWith(expect.objectContaining({ detailRunId: 'child-1' }));
});

test.each<[string, ConsoleApplicationLogRecordOverview['messages']]>([
  ['running', [{ role: 'system', content: 'Parent context', sequence: 1 }, { role: 'user', content: 'Real parent question', sequence: 2 }]],
  ['succeeded', [{ role: 'system', content: 'Parent context', sequence: 1 }]],
  ['succeeded', []]
])('a %s parent without an answer retains the child entrance without inventing conversation messages', async (status, messages) => {
  api.fetchApplicationLogRecordConversationMessages.mockImplementation(async (_app, _id, input) =>
    input.scope === 'children' ? page([record('child-1', { messages: [], parent_task_run_id: 'parent-1' })])
      : page([record('parent-1', { status, messages, child_task_count: 1 })])
  );
  const view = mount();
  const toggle = await screen.findByRole('button', { name: 'Subagents (1)' });
  expect(view.container.querySelector('.agent-flow-editor__debug-message--assistant')).toBeNull();
  expect(screen.queryByText('Question parent-1')).not.toBeInTheDocument();
  expect(screen.queryByText('Answer parent-1')).not.toBeInTheDocument();
  fireEvent.click(toggle);
  expect(await screen.findByText('No conversation messages recorded')).toBeInTheDocument();
  expect(view.container.querySelector('.agent-flow-editor__debug-message--assistant')).toBeNull();
});

test('child pagination preserves chronology and deduplicates overlaps; descendants load only on their own expansion', async () => {
  const child1 = record('child-1', { parent_task_run_id: 'parent-1', child_task_count: 1 });
  const child3 = record('child-3', { parent_task_run_id: 'parent-1' });
  api.fetchApplicationLogRecordConversationMessages.mockImplementation(async (_app, id, input) => {
    if (input.scope !== 'children') return page([record('parent-1', { child_task_count: 4 })]);
    if (id === 'child-1') return page([record('grandchild', { parent_task_run_id: 'child-1' })]);
    if (input.before) return page([child1, child3]);
    if (input.after) return page([record('child-4', { parent_task_run_id: 'parent-1' })]);
    return page([child3], { has_before: true, has_after: true, before_cursor: 'older-3', after_cursor: 'newer-3' });
  });
  const view = mount();
  fireEvent.click(await screen.findByRole('button', { name: 'Subagents (4)' }));
  await screen.findByText('Answer child-3');
  expect(screen.queryByText('Answer child-1')).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: 'Earlier subagents' }));
  await screen.findByText('Answer child-1');
  expect(screen.getAllByText('Answer child-3')).toHaveLength(1);
  expect(api.fetchApplicationLogRecordConversationMessages).toHaveBeenCalledWith(
    'app-1', 'parent-1', { scope: 'children', limit: 5, before: 'older-3' }
  );
  expect(api.fetchApplicationLogRecordConversationMessages.mock.calls.some((call) => call[1] === 'child-1')).toBe(false);
  fireEvent.click(screen.getByRole('button', { name: 'Later subagents' }));
  await screen.findByText('Answer child-4');
  expect([...childArea(view.container).querySelectorAll('[data-record-id]')].map((node) => node.getAttribute('data-record-id')))
    .toEqual(['child-1', 'child-3', 'child-4']);
  fireEvent.click(screen.getByRole('button', { name: 'Subagents (1)' }));
  await screen.findByText('Answer grandchild');
  expect(api.fetchApplicationLogRecordConversationMessages).toHaveBeenCalledWith(
    'app-1', 'child-1', { scope: 'children', limit: 5 }
  );
  expect(api.fetchApplicationLogRecordConversationMessages).toHaveBeenCalledWith(
    'app-1', 'parent-1', { scope: 'children', limit: 5, after: 'newer-3' }
  );
});

test('child failure and retry stay local while the parent transcript remains visible; collapse stops child rendering', async () => {
  let resolveChildren: (value: ReturnType<typeof page>) => void = () => {};
  api.fetchApplicationLogRecordConversationMessages.mockImplementation((_app, _id, input) => {
    if (input.scope !== 'children') return Promise.resolve(page([record('parent-1', { child_task_count: 1 })]));
    return Promise.reject(new Error('children unavailable'));
  });
  const view = mount();
  fireEvent.click(await screen.findByRole('button', { name: 'Subagents (1)' }));
  await screen.findByRole('alert');
  expect(screen.getByText('Answer parent-1')).toBeInTheDocument();
  api.fetchApplicationLogRecordConversationMessages.mockImplementation((_app, _id, input) => {
    if (input.scope !== 'children') return Promise.resolve(page([record('parent-1', { child_task_count: 1 })]));
    return new Promise((resolve) => { resolveChildren = resolve; });
  });
  fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
  await waitFor(() => expect(childArea(view.container).querySelector('.ant-spin')).not.toBeNull());
  resolveChildren(page([record('child-1', { parent_task_run_id: 'parent-1' })]));
  await screen.findByText('Answer child-1');
  fireEvent.click(screen.getByRole('button', { name: 'Subagents (1)' }));
  expect(screen.queryByText('Answer child-1')).not.toBeInTheDocument();
  expect(screen.getByText('Answer parent-1')).toBeInTheDocument();
});

test('expanded running children keep refreshing after their parent has succeeded', async () => {
  let childRequests = 0;
  api.fetchApplicationLogRecordConversationMessages.mockImplementation(async (_app, _id, input) => {
    if (input.scope !== 'children') return page([record('parent-1', { status: 'succeeded', child_task_count: 1 })]);
    childRequests += 1;
    return page([record('child-1', {
      parent_task_run_id: 'parent-1',
      status: childRequests === 1 ? 'running' : 'succeeded',
      messages: childRequests === 1
        ? [{ role: 'user', content: 'Child is working', sequence: 1 }]
        : [{ role: 'assistant', content: 'Child finished later', sequence: 2 }]
    })]);
  });
  mount();
  fireEvent.click(await screen.findByRole('button', { name: 'Subagents (1)' }));
  await screen.findByText('Child is working');
  expect(await screen.findByText('Child finished later', {}, { timeout: 2500 })).toBeInTheDocument();
  expect(childRequests).toBeGreaterThanOrEqual(2);
  expect(api.fetchApplicationLogRecordClientTrajectory).not.toHaveBeenCalled();
});

test('a message-less middle parent is identified and its child area stays between adjacent parent turns', async () => {
  api.fetchApplicationLogRecordConversationMessages.mockResolvedValue(page([
    record('parent-before'),
    record('parent-1', { title: 'Observed empty turn', messages: [], child_task_count: 1 }),
    record('parent-after')
  ]));
  const view = mount();
  const before = await screen.findByText('Answer parent-before');
  const after = screen.getByText('Question parent-after');
  const area = childArea(view.container);
  expect(within(area).getByText('Observed empty turn')).toBeInTheDocument();
  expect(before.compareDocumentPosition(area) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(area.compareDocumentPosition(after) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(api.fetchApplicationLogRecordConversationMessages).toHaveBeenCalledTimes(1);
});
