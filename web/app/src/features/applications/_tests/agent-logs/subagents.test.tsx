import '../../../../test/fixtures/rc-util-unique-ids';
import type { ConsoleApplicationLogRecordOverview } from '@1flowbase/api-client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen } from '@testing-library/react';
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

function record(
  id: string,
  overrides: Partial<ConsoleApplicationLogRecordOverview> = {}
): ConsoleApplicationLogRecordOverview {
  return {
    record_id: id,
    log_conversation_id: 'canonical-main-session',
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
      has_before: false,
      has_after: false,
      before_cursor: null,
      after_cursor: null,
      newest_cursor: 'latest',
      ...overrides
    }
  };
}
function mount(onOpenMessageLog = vi.fn()) {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <App>
        <ApplicationRunDetailPanel
          applicationId="app-1"
          runId="parent-2"
          recordId="parent-2"
          onClose={vi.fn()}
          onOpenMessageLog={onOpenMessageLog}
        />
      </App>
    </QueryClientProvider>
  );
}
beforeEach(async () => {
  vi.clearAllMocks();
  await appI18n.changeLanguage('en_US');
  api.fetchApplicationLogRecordClientTrajectory.mockResolvedValue({
    items: [],
    next_cursor: null,
    integrity: 'complete'
  });
});

test('L2 shows only the canonical main transcript even when a main task has children', async () => {
  api.fetchApplicationLogRecordConversationMessages.mockResolvedValue(
    page([
      record('main-latest', {
        child_task_count: 9,
        messages: [
          { role: 'system', content: 'Main system', sequence: 1 },
          { role: 'user', content: 'Main question', sequence: 2 },
          { role: 'assistant', content: 'Main final', sequence: 3 }
        ]
      })
    ])
  );
  const { container } = mount();
  expect(await screen.findByText('Main final')).toBeInTheDocument();
  expect(screen.getByText('Main question')).toBeInTheDocument();
  expect(screen.getByText('Main system')).toBeInTheDocument();
  expect(container.querySelector('[data-parent-record-id]')).toBeNull();
  expect(
    api.fetchApplicationLogRecordConversationMessages
  ).toHaveBeenCalledTimes(1);
  expect(
    api.fetchApplicationLogRecordConversationMessages
  ).toHaveBeenCalledWith('app-1', 'parent-2', { before: undefined, limit: 5 });
  expect(api.fetchApplicationLogRecordClientTrajectory).not.toHaveBeenCalled();
});
