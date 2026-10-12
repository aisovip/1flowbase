import '../../../../test/fixtures/rc-util-unique-ids';
import type { ReactNode } from 'react';
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within
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
function fixture(paginated = false) {
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
        previous_cursor:
          paginated && !refreshed && !query.cursor ? 'before' : null,
        next_cursor: paginated && !refreshed && !query.cursor ? 'after' : null,
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
    expect(screen.getByText('subagent', { selector: 'small' })).toHaveAttribute(
      'title',
      expect.stringContaining('codex · session · child')
    );
    expect(
      screen
        .getByRole('button', { name: '工具调用 · main-tool' })
        .querySelector('.conversation-trajectory__source')
    ).toBeNull();
    expect(f.loader.loadTree).not.toHaveBeenCalled();
    expect(f.loader.loadChildren).not.toHaveBeenCalled();
    expect(f.loader.loadRecordClientTrajectory).not.toHaveBeenCalled();
    expect(document.querySelector('.client-trajectory__request')).toBeNull();
  }
);
test('automatically loads both directions in server order and refresh restarts at original focus', async () => {
  const f = fixture(true);
  const { container } = mount(
    <ConversationTrajectoryWorkspace
      recordId="main-round"
      logConversationId="canonical"
      loader={f.loader}
    />
  );
  await screen.findByRole('button', { name: '工具调用 · main-tool' });
  expect(f.queryConversationClientTrajectory).toHaveBeenCalledTimes(1);
  expect(
    screen.queryByRole('button', { name: '加载更早事件' })
  ).not.toBeInTheDocument();
  await screen.findByRole('button', { name: '工具调用 · old-main' });
  expect(
    screen.queryByRole('button', { name: '加载更新事件' })
  ).not.toBeInTheDocument();
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
  fireEvent.click(screen.getByRole('button', { name: '子 Agent' }));
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
  fireEvent.click(screen.getByRole('button', { name: '工具调用' }));
  fireEvent.click(screen.getByRole('button', { name: '轨迹筛选' }));
  fireEvent.change(screen.getByLabelText('开始时间'), {
    target: { value: '2026-10-12T01:00:00' }
  });
  fireEvent.change(screen.getByLabelText('结束时间'), {
    target: { value: '2026-10-12T02:00:00' }
  });
  const search = screen.getByRole('textbox', { name: '搜索完整会话' });
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
  await waitFor(() =>
    expect(
      document.querySelector(
        '.conversation-trajectory__row .provider-trajectory__preview'
      )
    ).toHaveAttribute('title', '结果 · server-only result')
  );
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
  fireEvent.click(await screen.findByRole('button', { name: '工具结果' }));
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

test('original toolbar, loaded category chips, axis focus and explicit category grouping keep canonical filters', async () => {
  const f = fixture();
  const events = [
    {
      ...step('original-input', 'main-round'),
      category: 'request',
      created_at: '2026-10-12T00:00:00.123456Z',
      sequence: 1
    },
    {
      ...step('original-call', 'child', true),
      created_at: '2026-10-12T00:00:01.123456Z',
      sequence: 1
    },
    {
      ...step('original-answer', 'main-round'),
      category: 'assistant',
      created_at: '2026-10-12T00:00:02.123456Z',
      sequence: 1
    }
  ];
  f.queryConversationClientTrajectory.mockResolvedValue({
    log_conversation_id: 'canonical',
    items: events,
    matches: [],
    previous_cursor: null,
    next_cursor: null,
    search_sections: ['overview', 'parameters', 'result'],
    integrity: 'complete'
  });
  const { container } = mount(
    <ConversationTrajectoryWorkspace
      recordId="main-round"
      logConversationId="canonical"
      loader={f.loader}
    />
  );
  await screen.findByRole('button', { name: '工具调用 · original-call' });
  const categories = screen.getByRole('navigation', {
    name: '分类 · 已加载事件计数'
  });
  expect(
    within(categories).getByRole('button', { name: '工具调用' })
  ).toHaveTextContent('1');
  expect(screen.getByText('Responses')).toBeInTheDocument();
  expect(
    container.querySelectorAll('.provider-trajectory__lane-labels span')
  ).toHaveLength(3);
  expect(
    container.querySelector('.client-trajectory__category-group')
  ).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: '按分类归纳' }));
  expect(
    container.querySelectorAll('.client-trajectory__category-group')
  ).toHaveLength(3);
  expect(f.queryConversationClientTrajectory).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole('button', { name: '按分类归纳' }));
  expect(
    [...container.querySelectorAll('.conversation-trajectory__row strong')].map(
      (node) => node.textContent
    )
  ).toEqual(['original-input', 'original-call', 'original-answer']);
  const timeline = screen.getByRole('region', { name: '轨迹时间轴' });
  fireEvent.click(
    within(timeline).getByRole('button', { name: '工具调用 #1' })
  );
  await waitFor(() =>
    expect(
      container.querySelector('.provider-trajectory__inspector-header strong')
    ).toHaveTextContent('original-call')
  );
  expect(
    screen.getByRole('separator', { name: '步骤检查器' })
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: '关闭详情' }));
  fireEvent.click(within(categories).getByRole('button', { name: '工具调用' }));
  fireEvent.click(screen.getByRole('button', { name: '子 Agent' }));
  await waitFor(() =>
    expect(f.queryConversationClientTrajectory).toHaveBeenLastCalledWith(
      'canonical',
      expect.objectContaining({
        filter: { category: { $eq: 'tool_call' }, is_subagent: { $eq: true } }
      })
    )
  );
  await screen.findByRole('button', { name: '工具调用 · original-call' });
  const track = screen.getByTestId('trajectory-brush');
  track.getBoundingClientRect = () => ({ left: 0, width: 200 }) as DOMRect;
  track.setPointerCapture = vi.fn();
  vi.stubGlobal('PointerEvent', MouseEvent);
  fireEvent.pointerDown(track, { button: 0, clientX: 50, pointerId: 1 });
  fireEvent.pointerMove(track, { clientX: 150, pointerId: 1 });
  fireEvent.pointerUp(track, { clientX: 150, pointerId: 1 });
  await waitFor(() =>
    expect(f.queryConversationClientTrajectory).toHaveBeenLastCalledWith(
      'canonical',
      expect.objectContaining({
        focus_record_id: 'main-round',
        filter: {
          category: { $eq: 'tool_call' },
          is_subagent: { $eq: true },
          created_at: {
            $gte: '2026-10-12T00:00:00.623Z',
            $lt: '2026-10-12T00:00:01.624Z'
          }
        }
      })
    )
  );
  fireEvent.click(screen.getByRole('button', { name: '时间' }));
  expect(
    screen.getByText(
      '序号轴按已加载顺序映射时间范围；边界同时间戳事件一并显示。'
    )
  ).toBeInTheDocument();
  fireEvent.pointerDown(track, { button: 0, clientX: 150, pointerId: 2 });
  fireEvent.pointerMove(track, { clientX: 200, pointerId: 2 });
  fireEvent.pointerUp(track, { clientX: 200, pointerId: 2 });
  await waitFor(() =>
    expect(f.queryConversationClientTrajectory).toHaveBeenLastCalledWith(
      'canonical',
      expect.objectContaining({
        filter: {
          category: { $eq: 'tool_call' },
          is_subagent: { $eq: true },
          created_at: {
            $gte: '2026-10-12T00:00:02.123456Z',
            $lte: '2026-10-12T00:00:02.123456Z'
          }
        }
      })
    )
  );
  vi.unstubAllGlobals();
});

test('advanced category filter can query an event kind not present in the loaded chips', async () => {
  const f = fixture();
  f.queryConversationClientTrajectory.mockResolvedValue({
    log_conversation_id: 'canonical',
    items: [{ ...step('context-only', 'main-round'), category: 'context' }],
    matches: [],
    previous_cursor: null,
    next_cursor: 'unread',
    search_sections: ['overview', 'parameters', 'result'],
    integrity: 'complete'
  });
  mount(
    <ConversationTrajectoryWorkspace
      recordId="main-round"
      logConversationId="canonical"
      loader={f.loader}
    />
  );
  await screen.findByRole('button', { name: 'context · context-only' });
  expect(
    within(
      screen.getByRole('navigation', { name: '分类 · 已加载事件计数' })
    ).queryByRole('button', { name: '工具调用' })
  ).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: '轨迹筛选' }));
  fireEvent.mouseDown(await screen.findByRole('combobox', { name: '分类' }));
  fireEvent.click(
    await screen.findByText('工具调用', {
      selector: '.ant-select-item-option-content'
    })
  );
  await waitFor(() =>
    expect(f.queryConversationClientTrajectory).toHaveBeenLastCalledWith(
      'canonical',
      expect.objectContaining({
        focus_record_id: 'main-round',
        filter: { category: { $eq: 'tool_call' } }
      })
    )
  );
});

test('cancelled brush restores the committed selection and Tab cannot submit its draft', async () => {
  const f = fixture();
  const events = [
    {
      ...step('start', 'main-round'),
      created_at: '2026-10-12T00:00:00.123456Z'
    },
    {
      ...step('middle', 'child', true),
      created_at: '2026-10-12T00:00:01.123456Z'
    },
    { ...step('end', 'main-round'), created_at: '2026-10-12T00:00:02.123456Z' }
  ];
  f.queryConversationClientTrajectory.mockResolvedValue({
    log_conversation_id: 'canonical',
    items: events,
    matches: [],
    previous_cursor: null,
    next_cursor: null,
    search_sections: [],
    integrity: 'complete'
  });
  mount(
    <ConversationTrajectoryWorkspace
      recordId="main-round"
      logConversationId="canonical"
      loader={f.loader}
    />
  );
  await screen.findByRole('button', { name: '工具调用 · middle' });
  const track = screen.getByTestId('trajectory-brush');
  track.getBoundingClientRect = () => ({ left: 0, width: 200 }) as DOMRect;
  track.setPointerCapture = vi.fn();
  vi.stubGlobal('PointerEvent', MouseEvent);
  try {
    fireEvent.pointerDown(track, { button: 0, clientX: 50, pointerId: 1 });
    fireEvent.pointerMove(track, { clientX: 150, pointerId: 1 });
    expect(f.queryConversationClientTrajectory).toHaveBeenCalledTimes(1);
    fireEvent.pointerUp(track, { clientX: 150, pointerId: 1 });
    await waitFor(() =>
      expect(f.queryConversationClientTrajectory).toHaveBeenCalledTimes(2)
    );
    const start = screen.getByRole('slider', { name: '范围起点' });
    const end = screen.getByRole('slider', { name: '范围终点' });
    const committedStart = start.getAttribute('aria-valuenow'),
      committedEnd = end.getAttribute('aria-valuenow');
    fireEvent.pointerDown(track, { button: 0, clientX: 80, pointerId: 2 });
    fireEvent.pointerMove(track, { clientX: 120, pointerId: 2 });
    expect(start.getAttribute('aria-valuenow')).not.toBe(committedStart);
    fireEvent.pointerCancel(track, { pointerId: 2 });
    expect(start).toHaveAttribute('aria-valuenow', committedStart);
    expect(end).toHaveAttribute('aria-valuenow', committedEnd);
    fireEvent.keyDown(start, { key: 'Tab' });
    await act(async () => {
      await Promise.resolve();
    });
    expect(f.queryConversationClientTrajectory).toHaveBeenCalledTimes(2);
    expect(f.queryConversationClientTrajectory).toHaveBeenLastCalledWith(
      'canonical',
      expect.objectContaining({
        filter: {
          created_at: {
            $gte: '2026-10-12T00:00:00.623Z',
            $lt: '2026-10-12T00:00:01.624Z'
          }
        }
      })
    );
  } finally {
    vi.unstubAllGlobals();
  }
});

test('sequence selection maps boundary timestamps and retains server-returned timestamp peers', async () => {
  const f = fixture();
  const events = [
    {
      ...step('before', 'main-round'),
      created_at: '2026-10-12T00:00:00.123456789Z',
      sequence: 7
    },
    {
      ...step('selected-peer', 'child', true),
      created_at: '2026-10-12T00:00:01.123456789Z',
      sequence: 1
    },
    {
      ...step('same-time-peer', 'grandchild', true),
      created_at: '2026-10-12T00:00:01.123456789Z',
      sequence: 1
    }
  ];
  f.queryConversationClientTrajectory.mockImplementation(
    async (_canonical: string, query: ConversationTrajectoryQuery) => ({
      log_conversation_id: 'canonical',
      items: query.filter?.created_at ? events.slice(1) : events,
      matches: [],
      previous_cursor: null,
      next_cursor: null,
      search_sections: [],
      integrity: 'complete'
    })
  );
  const { container } = mount(
    <ConversationTrajectoryWorkspace
      recordId="main-round"
      logConversationId="canonical"
      loader={f.loader}
    />
  );
  await screen.findByRole('button', { name: '工具调用 · selected-peer' });
  fireEvent.click(screen.getByRole('button', { name: '时间' }));
  const track = screen.getByTestId('trajectory-brush');
  track.getBoundingClientRect = () => ({ left: 0, width: 200 }) as DOMRect;
  track.setPointerCapture = vi.fn();
  vi.stubGlobal('PointerEvent', MouseEvent);
  try {
    fireEvent.pointerDown(track, { button: 0, clientX: 90, pointerId: 1 });
    fireEvent.pointerMove(track, { clientX: 110, pointerId: 1 });
    fireEvent.pointerUp(track, { clientX: 110, pointerId: 1 });
    await waitFor(() =>
      expect(f.queryConversationClientTrajectory).toHaveBeenLastCalledWith(
        'canonical',
        expect.objectContaining({
          filter: {
            created_at: {
              $gte: '2026-10-12T00:00:01.123456789Z',
              $lte: '2026-10-12T00:00:01.123456789Z'
            }
          }
        })
      )
    );
    await waitFor(() =>
      expect(
        [
          ...container.querySelectorAll('.conversation-trajectory__row strong')
        ].map((node) => node.textContent)
      ).toEqual(['selected-peer', 'same-time-peer'])
    );
    expect(
      screen.getByText(
        '序号轴按已加载顺序映射时间范围；边界同时间戳事件一并显示。'
      )
    ).toBeInTheDocument();
  } finally {
    vi.unstubAllGlobals();
  }
});
