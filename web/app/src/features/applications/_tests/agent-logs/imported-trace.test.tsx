import '../../../../test/fixtures/rc-util-unique-ids';
import type { ReactNode } from 'react';
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
  ConversationClientTrajectoryStep,
  ConversationTrajectoryQuery,
  ConversationClientTrajectoryQueryPage
} from '@1flowbase/api-client';
import type { ConversationLogTraceLoader } from '../../../agent-flow/components/debug-console/conversation-log-trace-model';
import { ConversationLogPanel } from '../../../agent-flow/components/debug-console/ConversationLogPanel';
import { ProviderTrajectory } from '../../../agent-flow/components/debug-console/trajectory/ProviderTrajectory';
import { ConversationTrajectoryWorkspace } from '../../../agent-flow/components/debug-console/trajectory/client/ConversationTrajectoryWorkspace';
import { buildApplicationRunTraceMessage } from '../../components/logs/application-log-trace-loader';
import { appI18n } from '../../../../shared/i18n/app-i18n';
beforeEach(async () => {
  await appI18n.changeLanguage('zh_Hans');
});
function step(
  id: string,
  record_id: string,
  is_subagent = false
): ConversationClientTrajectoryStep {
  return {
    id,
    record_id,
    log_conversation_id: 'canonical',
    parent_task_run_id: is_subagent ? 'main-round' : null,
    is_subagent,
    source_kind: 'imported',
    source_id: 'source',
    source_client: 'codex',
    source_session_id: 'session',
    source_task_id: record_id,
    request_id: `request-${record_id}`,
    sequence: 1,
    created_at: '2026-10-12T00:00:00.123456Z',
    category: 'tool_call',
    name: id,
    namespace: null,
    preview: id,
    parameters_preview: null,
    result_preview: null,
    status: 'succeeded',
    origin: 'emitted',
    protocol: 'responses',
    transport: 'file',
    flow_run_id: null,
    node_run_id: null,
    parent_id: null,
    call_id: null,
    item_id: null,
    response_id: null,
    turn_id: null,
    related_step_id: id === 'main-tool' ? 'grandchild-tool' : null,
    available_sections: ['parameters', 'raw']
  };
}
function fixture() {
  let refreshed = false;
  const queryConversationClientTrajectory = vi
    .fn()
    .mockImplementation(
      async (_canonical: string, query: ConversationTrajectoryQuery) => ({
        log_conversation_id: 'canonical',
        items:
          query.cursor === 'before'
            ? [step('old-main', 'main-1')]
            : query.cursor === 'after'
              ? [step('grandchild-tool', 'grandchild', true)]
              : refreshed
                ? [step('late-backdated', 'main-1')]
                : [
                    step('main-tool', 'main-round'),
                    step('child-tool', 'child', true)
                  ],
        matches: query.keyword
          ? [
              {
                step_id: 'main-tool',
                section: 'result',
                sequence: 1,
                snippet: 'server-only result'
              }
            ]
          : [],
        previous_cursor: query.cursor ? null : 'before',
        next_cursor: query.cursor ? null : 'after',
        search_sections: query.search_sections ?? [],
        integrity: 'complete'
      })
    );
  const loadConversationClientTrajectorySection = vi
    .fn()
    .mockImplementation(async (_canonical, step_id, section) => ({
      step_id,
      section,
      request_id: 'owning-request',
      evidence_scope: section === 'raw' ? 'capture' : 'step',
      items: [
        {
          sequence: 0,
          value:
            section === 'raw'
              ? { body: 'exact archived body' }
              : 'own parameters'
        }
      ],
      next_cursor: null
    }));
  const loader: ConversationLogTraceLoader = {
    sourceKind: 'imported',
    queryConversationClientTrajectory,
    loadConversationClientTrajectorySection,
    loadRecordOverviewForTrajectory: vi.fn().mockResolvedValue({
      record_id: 'main-round',
      log_conversation_id: 'canonical'
    }),
    loadTree: vi.fn().mockResolvedValue({ nodes: [] }),
    loadChildren: vi.fn(),
    loadRecordClientTrajectory: vi.fn()
  };
  return {
    loader,
    queryConversationClientTrajectory,
    loadConversationClientTrajectorySection,
    refresh: () => {
      refreshed = true;
    }
  };
}
function mount(ui: ReactNode) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } }
  });
  const result = render(
    <QueryClientProvider client={client}>
      <App>{ui}</App>
    </QueryClientProvider>
  );
  return { ...result, client };
}
test.each(['message trace', 'trajectory window'])(
  '%s resolves canonical scope and shows one flat timeline without tree reads',
  async (entry) => {
    const f = fixture();
    mount(
      entry === 'message trace' ? (
        <ConversationLogPanel
          activeTab="trace"
          defaultTraceToolsExpanded
          onClose={vi.fn()}
          message={buildApplicationRunTraceMessage('main-round')}
          traceLoader={f.loader}
        />
      ) : (
        <ProviderTrajectory runId="main-round" loader={f.loader} />
      )
    );
    if (entry === 'trajectory window')
      fireEvent.click(screen.getByRole('button', { name: '总轨迹' }));
    await screen.findByRole('button', { name: '工具调用 · main-tool' });
    expect(f.queryConversationClientTrajectory).toHaveBeenCalledWith(
      'canonical',
      expect.objectContaining({
        focus_record_id: 'main-round',
        cursor: undefined
      })
    );
    expect(screen.getByText('子 Agent · codex · child')).toBeInTheDocument();
    expect(f.loader.loadTree).not.toHaveBeenCalled();
    expect(f.loader.loadChildren).not.toHaveBeenCalled();
    expect(f.loader.loadRecordClientTrajectory).not.toHaveBeenCalled();
    expect(document.querySelector('.client-trajectory__request')).toBeNull();
  }
);
test('previous and next are on demand, retain scope and server ordering, refresh discards pages at original focus', async () => {
  const f = fixture();
  const { container } = mount(
    <ConversationTrajectoryWorkspace
      recordId="main-round"
      logConversationId="canonical"
      loader={f.loader}
    />
  );
  await screen.findByRole('button', { name: '工具调用 · main-tool' });
  expect(f.queryConversationClientTrajectory).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole('button', { name: '加载更早事件' }));
  await screen.findByRole('button', { name: '工具调用 · old-main' });
  fireEvent.click(screen.getByRole('button', { name: '加载更新事件' }));
  await screen.findByRole('button', { name: '工具调用 · grandchild-tool' });
  expect(
    [...container.querySelectorAll('.conversation-trajectory__row strong')].map(
      (node) => node.textContent
    )
  ).toEqual(['old-main', 'main-tool', 'child-tool', 'grandchild-tool']);
  for (const [, query] of f.queryConversationClientTrajectory.mock.calls)
    expect(query.focus_record_id).toBe('main-round');
  f.refresh();
  fireEvent.click(screen.getByRole('button', { name: /刷\s*新/ }));
  await screen.findByRole('button', { name: '工具调用 · late-backdated' });
  expect(
    screen.queryByRole('button', { name: '工具调用 · grandchild-tool' })
  ).not.toBeInTheDocument();
  expect(
    screen.queryByRole('button', { name: '工具调用 · old-main' })
  ).not.toBeInTheDocument();
  expect(f.queryConversationClientTrajectory).toHaveBeenLastCalledWith(
    'canonical',
    expect.objectContaining({
      focus_record_id: 'main-round',
      cursor: undefined
    })
  );
});
test('category, provenance, time and semantic keyword are sent to the server without filtering its rows', async () => {
  const f = fixture();
  mount(
    <ConversationTrajectoryWorkspace
      recordId="main-round"
      logConversationId="canonical"
      loader={f.loader}
    />
  );
  await screen.findByRole('button', { name: '工具调用 · main-tool' });
  fireEvent.mouseDown(screen.getByRole('combobox', { name: '来源' }));
  fireEvent.click(
    await screen.findByText('子 Agent', {
      selector: '.ant-select-item-option-content'
    })
  );
  await waitFor(() =>
    expect(f.queryConversationClientTrajectory).toHaveBeenLastCalledWith(
      'canonical',
      expect.objectContaining({ filter: { is_subagent: { $eq: true } } })
    )
  );
  // A main row returned by the fixture remains visible: filtering truth belongs to the backend.
  expect(
    await screen.findByRole('button', { name: '工具调用 · main-tool' })
  ).toBeInTheDocument();
  fireEvent.mouseDown(screen.getByRole('combobox', { name: '分类' }));
  fireEvent.click(
    await screen.findByText('工具调用', {
      selector: '.ant-select-item-option-content'
    })
  );
  fireEvent.change(screen.getByLabelText('开始时间'), {
    target: { value: '2026-10-12T01:00:00' }
  });
  fireEvent.change(screen.getByLabelText('结束时间'), {
    target: { value: '2026-10-12T02:00:00' }
  });
  const search = screen.getByRole('searchbox', { name: '搜索完整会话' });
  fireEvent.change(search, { target: { value: 'unread-result' } });
  fireEvent.keyDown(search, { key: 'Enter', code: 'Enter' });
  await waitFor(() =>
    expect(f.queryConversationClientTrajectory).toHaveBeenLastCalledWith(
      'canonical',
      expect.objectContaining({
        keyword: 'unread-result',
        search_sections: ['overview', 'parameters', 'result'],
        filter: {
          is_subagent: { $eq: true },
          category: { $eq: 'tool_call' },
          created_at: {
            $gte: new Date('2026-10-12T01:00:00').toISOString(),
            $lte: new Date('2026-10-12T02:00:00').toISOString()
          }
        }
      })
    )
  );
  await screen.findByText('结果 · server-only result');
});
test('lazy detail and raw sections use canonical membership, not a child record route', async () => {
  const f = fixture();
  mount(
    <ConversationTrajectoryWorkspace
      recordId="main-round"
      logConversationId="canonical"
      loader={f.loader}
    />
  );
  const child = await screen.findByRole('button', {
    name: '工具调用 · child-tool'
  });
  expect(f.loadConversationClientTrajectorySection).not.toHaveBeenCalled();
  fireEvent.click(child);
  await waitFor(() =>
    expect(f.loadConversationClientTrajectorySection).toHaveBeenCalledWith(
      'canonical',
      'child-tool',
      'parameters',
      undefined
    )
  );
  expect(
    f.loadConversationClientTrajectorySection.mock.calls.some(
      (call) => call[2] === 'raw'
    )
  ).toBe(false);
  fireEvent.click(screen.getByRole('tab', { name: '原始协议' }));
  await waitFor(() =>
    expect(f.loadConversationClientTrajectorySection).toHaveBeenCalledWith(
      'canonical',
      'child-tool',
      'raw',
      undefined
    )
  );
});
test('assistant explicit canonical identity avoids a lookup and unresolved identity never guesses a namespace', async () => {
  const f = fixture();
  const first = mount(
    <ProviderTrajectory
      runId="main-round"
      logConversationId="canonical"
      loader={f.loader}
    />
  );
  fireEvent.click(screen.getByRole('button', { name: '总轨迹' }));
  await screen.findByRole('button', { name: '工具调用 · main-tool' });
  expect(f.loader.loadRecordOverviewForTrajectory).not.toHaveBeenCalled();
  first.unmount();
  f.queryConversationClientTrajectory.mockClear();
  f.loader.loadRecordOverviewForTrajectory = vi
    .fn()
    .mockResolvedValue({ record_id: 'main-round', log_conversation_id: null });
  mount(
    <ConversationTrajectoryWorkspace recordId="main-round" loader={f.loader} />
  );
  await screen.findByText('无法获取规范会话标识。');
  expect(f.queryConversationClientTrajectory).not.toHaveBeenCalled();
});

test('related event outside the category and page opens through a separate canonical lookup', async () => {
  const f = fixture();
  let resolveRelated!: (value: ConversationClientTrajectoryQueryPage) => void;
  const pendingRelated = new Promise<ConversationClientTrajectoryQueryPage>(
    (resolve) => {
      resolveRelated = resolve;
    }
  );
  f.queryConversationClientTrajectory.mockImplementation(
    async (_canonical: string, query: ConversationTrajectoryQuery) => {
      if (query.filter?.id) return pendingRelated;
      return {
        log_conversation_id: 'canonical',
        items: [
          {
            ...step('main-tool', 'main-round'),
            category: 'tool_result',
            related_step_id: 'related-call'
          }
        ],
        matches: [],
        previous_cursor: null,
        next_cursor: null,
        search_sections: query.search_sections ?? [],
        integrity: 'complete'
      };
    }
  );
  const { container } = mount(
    <ConversationTrajectoryWorkspace
      recordId="main-round"
      logConversationId="canonical"
      loader={f.loader}
    />
  );
  await screen.findByRole('button', { name: '工具结果 · main-tool' });
  fireEvent.mouseDown(screen.getByRole('combobox', { name: '分类' }));
  fireEvent.click(
    await screen.findByText('工具结果', {
      selector: '.ant-select-item-option-content'
    })
  );
  fireEvent.click(
    await screen.findByRole('button', { name: '工具结果 · main-tool' })
  );
  fireEvent.click(screen.getByRole('button', { name: '定位关联调用' }));
  await waitFor(() =>
    expect(f.queryConversationClientTrajectory).toHaveBeenLastCalledWith(
      'canonical',
      { filter: { id: { $eq: 'related-call' } }, limit: 1 }
    )
  );
  expect(
    container.querySelector('.provider-trajectory__inspector-header strong')
  ).toHaveTextContent('main-tool');
  await act(async () =>
    resolveRelated({
      log_conversation_id: 'canonical',
      items: [step('related-call', 'child', true)],
      matches: [],
      previous_cursor: null,
      next_cursor: null,
      search_sections: [],
      integrity: 'complete'
    })
  );
  await waitFor(() =>
    expect(
      container.querySelector('.provider-trajectory__inspector-header strong')
    ).toHaveTextContent('related-call')
  );
  expect(
    screen.queryByRole('button', { name: '工具调用 · related-call' })
  ).not.toBeInTheDocument();
  expect(
    [...container.querySelectorAll('.conversation-trajectory__row strong')].map(
      (node) => node.textContent
    )
  ).toEqual(['main-tool']);
  expect(f.queryConversationClientTrajectory).toHaveBeenCalledWith(
    'canonical',
    expect.objectContaining({ filter: { category: { $eq: 'tool_result' } } })
  );
  await waitFor(() =>
    expect(f.loadConversationClientTrajectorySection).toHaveBeenCalledWith(
      'canonical',
      'related-call',
      'parameters',
      undefined
    )
  );
});
