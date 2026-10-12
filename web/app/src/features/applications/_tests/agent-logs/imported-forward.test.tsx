import '../../../../test/fixtures/rc-util-unique-ids';
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor
} from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { App } from 'antd';
import { beforeEach, expect, test, vi } from 'vitest';
import type {
  ConsoleApplicationLogRecordOverview,
  ApplicationLogRecordConversationPage
} from '@1flowbase/api-client';
import { ApplicationRunDetailPanel } from '../../components/logs/ApplicationRunDetailPanel';
import { appI18n } from '../../../../shared/i18n/app-i18n';

const api = vi.hoisted(() => ({
  fetchApplicationLogRecordConversationMessages: vi.fn()
}));
vi.mock('../../api/trajectory', async (original) => ({
  ...(await original<typeof import('../../api/trajectory')>()),
  ...api
}));
beforeEach(async () => {
  api.fetchApplicationLogRecordConversationMessages.mockReset();
  await appI18n.changeLanguage('zh_Hans');
});
const turn = (number: number): ConsoleApplicationLogRecordOverview => ({
  record_id: 'record-' + number,
  log_conversation_id: 'canonical-session',
  parent_task_run_id: null,
  child_task_count: number === 7 ? 9 : 0,
  status: 'succeeded',
  source_id: 'source',
  source_kind: 'imported',
  source_client: 'codex',
  source_session_id: 'main-session',
  source_task_id: 'main-turn-' + number,
  native_run_id: null,
  title: 'Main question ' + number,
  outcome: 'final_answer_observed',
  started_at: new Date(Date.UTC(2026, 9, 12, 0, 0, number)).toISOString(),
  finished_at: null,
  requested_model_id: null,
  reasoning_effort: null,
  messages: [
    { role: 'system', content: 'Main system ' + number, sequence: 1 },
    { role: 'user', content: 'Main question ' + number, sequence: 2 },
    { role: 'assistant', content: 'Main final ' + number, sequence: 3 }
  ],
  total_tokens: 10,
  input_tokens: 7,
  output_tokens: 3,
  input_cache_hit_tokens: 0,
  cost_breakdown: { total_cost: '0' },
  available_views: ['conversation', 'client_trajectory']
});
function page(
  numbers: number[],
  has_after = false
): ApplicationLogRecordConversationPage {
  return {
    items: numbers.map(turn),
    page: {
      has_before: numbers[0] > 1,
      has_after,
      before_cursor: 'cursor-' + numbers[0],
      after_cursor: has_after ? 'cursor-' + numbers.at(-1) : null,
      newest_cursor: 'cursor-' + numbers.at(-1)
    }
  };
}
function mount() {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <App>
        <ApplicationRunDetailPanel
          applicationId="app"
          runId="child-anchor"
          recordId="child-anchor"
          onClose={vi.fn()}
        />
      </App>
    </QueryClientProvider>
  );
}

test.each([false, true])(
  'main transcript retains loaded older turns and catches more than five arrivals (first after request fails: %s)',
  async (failFirstAfter) => {
    // Timers control only refresh intervals; UI/query scheduling remains real.
    vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] });
    let summaryReads = 0;
    let failed = false;
    api.fetchApplicationLogRecordConversationMessages.mockImplementation(
      async (_app, _anchor, input) => {
        if (input.before) return page([1, 2]);
        if (input.after === 'cursor-7') {
          if (failFirstAfter && !failed) {
            failed = true;
            throw new Error('temporary forward failure');
          }
          return page([8, 9, 10, 11, 12], true);
        }
        if (input.after === 'cursor-12') return page([13, 14, 15]);
        summaryReads += 1;
        return summaryReads === 1
          ? page([3, 4, 5, 6, 7])
          : page([11, 12, 13, 14, 15]);
      }
    );
    const view = mount();
    try {
      expect(await screen.findByText('Main final 7')).toBeInTheDocument();
      expect(screen.queryByText('Main final 1')).not.toBeInTheDocument();
      expect(
        api.fetchApplicationLogRecordConversationMessages.mock.calls.some(
          (call) => call[2].before
        )
      ).toBe(false);
      fireEvent.scroll(screen.getByTestId('debug-conversation-messages'), {
        target: { scrollTop: 0 }
      });
      expect(await screen.findByText('Main final 1')).toBeInTheDocument();
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1_000);
      });
      if (failFirstAfter) {
        await screen.findByRole('alert');
        expect(screen.getByText('Main final 3')).toBeInTheDocument();
        await act(async () => {
          await vi.advanceTimersByTimeAsync(1_000);
        });
      }
      expect(await screen.findByText('Main final 8')).toBeInTheDocument();
      for (let number = 1; number <= 15; number += 1) {
        expect(screen.getAllByText('Main system ' + number)).toHaveLength(1);
        expect(screen.getAllByText('Main question ' + number)).toHaveLength(1);
        expect(screen.getAllByText('Main final ' + number)).toHaveLength(1);
      }
      expect(
        api.fetchApplicationLogRecordConversationMessages
      ).toHaveBeenCalledWith('app', 'child-anchor', {
        after: 'cursor-7',
        limit: 5
      });
      expect(
        api.fetchApplicationLogRecordConversationMessages
      ).toHaveBeenCalledWith('app', 'child-anchor', {
        after: 'cursor-12',
        limit: 5
      });
      expect(
        api.fetchApplicationLogRecordConversationMessages.mock.calls
          .filter((call) => call[2].before)
          .map((call) => call[2].before)
      ).toEqual(['cursor-3']);
      expect(screen.getByText('Main final 2')).toBeInTheDocument();
      expect(
        view.container.querySelector('[data-parent-record-id]')
      ).toBeNull();
      expect(
        api.fetchApplicationLogRecordConversationMessages.mock.calls.every(
          (call) => !('scope' in call[2])
        )
      ).toBe(true);
    } finally {
      view.unmount();
      vi.useRealTimers();
    }
  }
);

test('a deferred after request survives multiple summary polls and drains later pages without parallel requests', async () => {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] });
  let releaseForward!: (page: ApplicationLogRecordConversationPage) => void;
  const pendingForward = new Promise<ApplicationLogRecordConversationPage>(
    (resolve) => {
      releaseForward = resolve;
    }
  );
  let summaryReads = 0;
  api.fetchApplicationLogRecordConversationMessages.mockImplementation(
    async (_app, _anchor, input) => {
      if (input.before) return page([1, 2]);
      if (input.after === 'cursor-7') return pendingForward;
      if (input.after === 'cursor-12') return page([13, 14, 15, 16, 17], true);
      if (input.after === 'cursor-17') return page([18, 19, 20, 21, 22], true);
      if (input.after === 'cursor-22') return page([23]);
      summaryReads += 1;
      if (summaryReads === 1) return page([3, 4, 5, 6, 7]);
      return summaryReads === 2
        ? page([11, 12, 13, 14, 15])
        : page([19, 20, 21, 22, 23]);
    }
  );
  const view = mount();
  try {
    await screen.findByText('Main final 7');
    fireEvent.scroll(screen.getByTestId('debug-conversation-messages'), {
      target: { scrollTop: 0 }
    });
    await screen.findByText('Main final 1');
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_000);
    });
    await waitFor(() =>
      expect(
        api.fetchApplicationLogRecordConversationMessages
      ).toHaveBeenCalledWith('app', 'child-anchor', {
        after: 'cursor-7',
        limit: 5
      })
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2_000);
    });
    await screen.findByText('Main final 23');
    expect(summaryReads).toBeGreaterThanOrEqual(3);
    expect(screen.queryByText('Main final 8')).not.toBeInTheDocument();
    expect(
      api.fetchApplicationLogRecordConversationMessages.mock.calls
        .filter((call) => call[2].after)
        .map((call) => call[2].after)
    ).toEqual(['cursor-7']);
    await act(async () => {
      releaseForward(page([8, 9, 10, 11, 12], true));
      await pendingForward;
    });
    await screen.findByText('Main final 8');
    await screen.findByText('Main final 18');
    for (let number = 1; number <= 23; number += 1) {
      expect(screen.getAllByText('Main final ' + number)).toHaveLength(1);
    }
    expect(
      api.fetchApplicationLogRecordConversationMessages.mock.calls
        .filter((call) => call[2].after)
        .map((call) => call[2].after)
    ).toEqual(['cursor-7', 'cursor-12', 'cursor-17', 'cursor-22']);
    expect(
      api.fetchApplicationLogRecordConversationMessages.mock.calls
        .filter((call) => call[2].before)
        .map((call) => call[2].before)
    ).toEqual(['cursor-3']);
    expect(view.container.querySelector('[data-parent-record-id]')).toBeNull();
  } finally {
    view.unmount();
    vi.useRealTimers();
  }
});
