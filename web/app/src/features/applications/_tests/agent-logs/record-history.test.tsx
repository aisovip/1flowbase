import '../../../../test/fixtures/rc-util-unique-ids';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { App } from 'antd';
import { beforeEach, expect, test, vi } from 'vitest';
import type { ConsoleApplicationLogRecordOverview } from '@1flowbase/api-client';
import type { AgentFlowDebugMessage } from '../../../agent-flow/api/runtime';
import { appI18n } from '../../../../shared/i18n/app-i18n';
import { ApplicationRunDetailPanel } from '../../components/logs/ApplicationRunDetailPanel';

const api = vi.hoisted(() => ({
  fetchApplicationLogRecordConversationMessages: vi.fn()
}));
vi.mock('../../api/trajectory', async (original) => ({
  ...(await original<typeof import('../../api/trajectory')>()),
  ...api
}));
vi.mock(
  '../../../agent-flow/components/debug-console/AgentFlowDebugConsole',
  () => ({
    AgentFlowDebugConsole: ({
      messages,
      onReachConversationTop,
      onOpenMessageLog
    }: {
      messages: AgentFlowDebugMessage[];
      onReachConversationTop: () => void;
      onOpenMessageLog: (message: AgentFlowDebugMessage) => void;
    }) => (
      <section>
        <button onClick={onReachConversationTop}>older turns</button>
        {messages.map((message) => (
          <article key={message.id} data-testid={message.id}>
            <span>{message.content}</span>
            {message.role === 'assistant' && message.canOpenDetail && (
              <button onClick={() => onOpenMessageLog(message)}>
                trace {message.content}
              </button>
            )}
          </article>
        ))}
      </section>
    )
  })
);
const turn = (number: number): ConsoleApplicationLogRecordOverview => ({
  parent_task_run_id: null,
  child_task_count: 0,
  log_conversation_id: 'canonical-main-session',
  record_id: 'record-' + number,
  status: 'succeeded',
  source_id: 'source-1',
  source_kind: 'imported',
  source_client: 'deepseek-harness',
  source_session_id: 'session-1',
  source_task_id: 'turn-' + number,
  native_run_id: null,
  title: 'question ' + number,
  outcome: 'final_answer_observed',
  started_at: new Date(Date.UTC(2026, 9, 12, 0, 0, number)).toISOString(),
  finished_at: null,
  requested_model_id: 'gpt-6-luna',
  reasoning_effort: 'max',
  messages: [
    { role: 'system', content: 'system ' + number, sequence: 1 },
    { role: 'user', content: 'question ' + number, sequence: 2 },
    { role: 'assistant', content: 'answer ' + number, sequence: 8 }
  ],
  total_tokens: 10,
  input_tokens: 7,
  output_tokens: 3,
  input_cache_hit_tokens: 0,
  cost_breakdown: { total_cost: '0' },
  available_views: ['conversation', 'client_trajectory']
});
const page = (numbers: number[], hasBefore: boolean) => ({
  items: numbers.map(turn),
  page: {
    has_before: hasBefore,
    has_after: false,
    before_cursor: 'cursor-' + numbers[0],
    after_cursor: null,
    newest_cursor: 'cursor-' + numbers.at(-1)
  }
});
function mount(onOpenMessageLog = vi.fn()) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } }
  });
  return render(
    <QueryClientProvider client={client}>
      <App>
        <ApplicationRunDetailPanel
          applicationId="app-1"
          runId="record-7"
          recordId="record-7"
          onClose={vi.fn()}
          onOpenMessageLog={onOpenMessageLog}
        />
      </App>
    </QueryClientProvider>
  );
}
beforeEach(async () => {
  api.fetchApplicationLogRecordConversationMessages.mockReset();
  await appI18n.changeLanguage('zh_Hans');
});
test('five backend business turns prepend older history and each answer opens its own record', async () => {
  api.fetchApplicationLogRecordConversationMessages.mockImplementation(
    async (_app, _record, input) =>
      input.before ? page([1, 2, 3], false) : page([3, 4, 5, 6, 7], true)
  );
  const open = vi.fn();
  mount(open);
  expect(await screen.findByText('answer 7')).toBeInTheDocument();
  expect(screen.queryByText('answer 2')).not.toBeInTheDocument();
  expect(
    api.fetchApplicationLogRecordConversationMessages
  ).toHaveBeenCalledWith('app-1', 'record-7', { before: undefined, limit: 5 });
  fireEvent.click(screen.getByRole('button', { name: 'older turns' }));
  expect(await screen.findByText('answer 1')).toBeInTheDocument();
  expect(
    api.fetchApplicationLogRecordConversationMessages
  ).toHaveBeenLastCalledWith('app-1', 'record-7', {
    before: 'cursor-3',
    limit: 5
  });
  expect(screen.getAllByText('answer 3')).toHaveLength(1);
  expect(
    screen
      .getAllByRole('article')
      .map((element) => element.textContent?.split('trace')[0])
  ).toEqual(
    [1, 2, 3, 4, 5, 6, 7].flatMap((n) => [
      'system ' + n,
      'question ' + n,
      'answer ' + n
    ])
  );
  fireEvent.click(screen.getByRole('button', { name: 'trace answer 2' }));
  expect(open).toHaveBeenLastCalledWith(
    expect.objectContaining({
      detailRunId: 'record-2',
      runId: null,
      content: 'answer 2'
    })
  );
  fireEvent.click(screen.getByRole('button', { name: 'trace answer 7' }));
  expect(open).toHaveBeenLastCalledWith(
    expect.objectContaining({ detailRunId: 'record-7' })
  );
  fireEvent.click(screen.getByRole('button', { name: 'older turns' }));
  expect(
    api.fetchApplicationLogRecordConversationMessages.mock.calls.filter(
      (call) => call[2].before
    )
  ).toHaveLength(1);
});
test('older-page failure stays visible without losing already loaded conversation', async () => {
  api.fetchApplicationLogRecordConversationMessages.mockImplementation(
    async (_app, _record, input) => {
      if (input.before) throw new Error('history unavailable');
      return page([3, 4, 5, 6, 7], true);
    }
  );
  mount();
  expect(await screen.findByText('answer 7')).toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: 'older turns' }));
  await waitFor(() => expect(screen.getByRole('alert')).toBeInTheDocument());
  expect(screen.getByText('answer 7')).toBeInTheDocument();
});
