import './navigation';
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within
} from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { beforeEach, expect, test, vi } from 'vitest';
import type {
  ClientTrajectoryStep,
  ConversationClientTrajectoryStep,
  ConversationTrajectoryQuery
} from '@1flowbase/api-client';
import { ProviderTrajectory } from '../../../components/debug-console/trajectory/ProviderTrajectory';
import type { ConversationLogTraceLoader } from '../../../components/debug-console/conversation-log-trace-model';
import { appI18n } from '../../../../../shared/i18n/app-i18n';
beforeEach(async () => {
  await appI18n.changeLanguage('zh_Hans');
});
const root: ClientTrajectoryStep = {
  id: 'request-1',
  request_id: 'request-1',
  sequence: 1,
  created_at: '2026-09-22T00:00:00Z',
  category: 'request',
  name: 'Responses request',
  namespace: null,
  preview: 'gpt-5.6-luna · max',
  parameters_preview: null,
  result_preview: null,
  status: 'submitted',
  origin: 'submitted',
  protocol: 'responses',
  transport: 'http',
  flow_run_id: 'run-1',
  node_run_id: null,
  parent_id: null,
  call_id: null,
  item_id: null,
  response_id: null,
  turn_id: null,
  related_step_id: null,
  available_sections: ['overview', 'timing', 'raw']
};
const tool: ClientTrajectoryStep = {
  ...root,
  id: 'call-1',
  sequence: 2,
  parent_id: root.id,
  category: 'tool_call',
  name: 'exec_command',
  preview: 'pwd',
  parameters_preview: '{"cmd":"pwd"}',
  call_id: 'call-original',
  origin: 'emitted',
  available_sections: ['overview', 'parameters', 'schema', 'timing', 'raw']
};
function fixture(namespace: string | null = null, protocol = 'responses') {
  const loadClientTrajectory = vi
    .fn()
    .mockImplementation((_run, _node, cursor) =>
      Promise.resolve({
        items: cursor
          ? [
              {
                ...root,
                id: 'result-1',
                sequence: 3,
                category: 'tool_result',
                name: 'exec_command',
                result_preview: '/work',
                related_step_id: tool.id,
                call_id: tool.call_id,
                available_sections: ['result']
              }
            ]
          : [
              { ...root, protocol },
              { ...tool, namespace, protocol }
            ],
        next_cursor: cursor ? null : 2,
        integrity: 'complete'
      })
    );
  const loadClientTrajectorySection = vi
    .fn()
    .mockImplementation((_run, stepId, section) =>
      Promise.resolve({
        step_id: stepId,
        request_id: root.id,
        evidence_scope: section === 'raw' ? 'capture' : 'step',
        section,
        items: [
          {
            sequence: 1,
            value:
              section === 'parameters'
                ? '  {"cmd":"pwd"}  '
                : section === 'raw'
                  ? {
                      direction: 'submitted',
                      encoding: 'utf8',
                      body: '  { "input": "客户端原文" }\n',
                      frame_kind: 'request'
                    }
                  : { model: 'gpt-5.6-luna' }
          }
        ],
        next_cursor: null
      })
    );
  const loadWorkflowTrajectory = vi
    .fn()
    .mockResolvedValue({ items: [], next_cursor: null });
  const loader: ConversationLogTraceLoader = {
    loadTree: vi.fn(),
    loadChildren: vi.fn(),
    loadContent: vi.fn(),
    loadClientTrajectory,
    loadClientTrajectorySection,
    loadWorkflowTrajectory
  };
  render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <ProviderTrajectory runId="run-1" loader={loader} />
    </QueryClientProvider>
  );
  return {
    loadClientTrajectory,
    loadClientTrajectorySection,
    loadWorkflowTrajectory
  };
}
test('defaults to original client classification and lazily loads only selected sections', async () => {
  const {
    loadClientTrajectory,
    loadClientTrajectorySection,
    loadWorkflowTrajectory
  } = fixture();
  expect(loadClientTrajectory).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: '总轨迹' }));
  const call = await screen.findByRole('button', {
    name: '工具调用 · exec_command'
  });
  expect(loadWorkflowTrajectory).toHaveBeenCalledTimes(1);
  expect(loadClientTrajectorySection).not.toHaveBeenCalled();
  fireEvent.click(call);
  await waitFor(() =>
    expect(loadClientTrajectorySection).toHaveBeenCalledWith(
      'run-1',
      'call-1',
      'overview',
      undefined,
      undefined
    )
  );
  const inspector = screen.getByRole('complementary', { name: '步骤检查器' });
  expect(inspector.parentElement).toBe(
    call.closest('.provider-trajectory__split')
  );
  fireEvent.click(within(inspector).getByRole('tab', { name: '参数' }));
  const parameters = await within(inspector).findByText('{"cmd":"pwd"}', {
    selector: 'pre'
  });
  // eslint-disable-next-line jest-dom/prefer-to-have-text-content -- Exact raw whitespace is the protocol contract; substring matching is insufficient.
  expect(parameters.textContent).toBe('  {"cmd":"pwd"}  ');
  expect(
    loadClientTrajectorySection.mock.calls.some((args) => args[2] === 'raw')
  ).toBe(false);
  fireEvent.click(within(inspector).getByRole('tab', { name: '原始协议' }));
  await waitFor(() =>
    expect(loadClientTrajectorySection).toHaveBeenCalledWith(
      'run-1',
      'call-1',
      'raw',
      undefined,
      undefined
    )
  );
  // eslint-disable-next-line jest-dom/prefer-to-have-text-content -- Preserve exact raw protocol bytes, including the trailing newline.
  expect((await screen.findByText(/客户端原文/)).textContent).toBe(
    '  { "input": "客户端原文" }\n'
  );
  expect(within(inspector).getByText('utf8')).toBeInTheDocument();
  expect(within(inspector).getByText('request')).toBeInTheDocument();
  fireEvent.click(call);
  expect(screen.getByRole('complementary')).toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: '关闭详情' }));
  expect(screen.queryByRole('complementary')).not.toBeInTheDocument();
});
test('paginates summaries, keeps original categories and follows actual call relation', async () => {
  const { loadClientTrajectory } = fixture();
  fireEvent.click(screen.getByRole('button', { name: '总轨迹' }));
  await screen.findByRole('button', { name: '工具调用 · exec_command' });
  const result = await screen.findByRole('button', {
    name: '工具结果 · exec_command'
  });
  expect(loadClientTrajectory).toHaveBeenCalledWith(
    'run-1',
    undefined,
    2,
    undefined
  );
  fireEvent.click(result);
  fireEvent.click(await screen.findByRole('button', { name: '定位关联调用' }));
  expect(
    screen.getByRole('button', { name: '工具调用 · exec_command' })
  ).toHaveAttribute('aria-pressed', 'true');
  fireEvent.click(screen.getByRole('button', { name: '收起请求' }));
  expect(
    screen.queryByRole('button', { name: '工具调用 · exec_command' })
  ).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: '展开请求' }));
  expect(
    screen.getByRole('button', { name: '工具调用 · exec_command' })
  ).toBeInTheDocument();
});

test('shows and searches actual protocol namespace without replacing the original tool name', async () => {
  fixture('mcp__codex_apps__github');
  fireEvent.click(screen.getByRole('button', { name: '总轨迹' }));
  const row = await screen.findByRole('button', {
    name: '工具调用 · mcp__codex_apps__github.exec_command'
  });
  fireEvent.change(screen.getByRole('textbox', { name: '搜索已加载步骤' }), {
    target: { value: 'mcp__codex_apps__github' }
  });
  expect(row).toBeInTheDocument();
  fireEvent.click(row);
  const inspector = screen.getByRole('complementary', { name: '步骤检查器' });
  expect(within(inspector).getByText('namespace')).toBeInTheDocument();
  expect(
    within(inspector).getByText('mcp__codex_apps__github')
  ).toBeInTheDocument();
});

test('Chat toolbar uses backend protocol classification', async () => {
  fixture(null, 'chat_completions');
  fireEvent.click(screen.getByRole('button', { name: '总轨迹' }));
  expect(await screen.findByText('Chat Completions')).toBeInTheDocument();
  expect(
    screen.queryByText('Responses', {
      selector: '.client-trajectory__protocol'
    })
  ).not.toBeInTheDocument();
});

test('imported records reuse total trajectory and lazy sections without a native run or internal calls', async () => {
  const queryConversationClientTrajectory = vi.fn().mockResolvedValue({
    log_conversation_id: 'canonical-1',
    items: [
      {
        ...tool,
        record_id: 'record-1',
        log_conversation_id: 'canonical-1',
        parent_task_run_id: null,
        is_subagent: false,
        source_kind: 'imported',
        source_id: 'source-1',
        source_client: 'codex',
        source_session_id: 'session-1',
        source_task_id: 'task-1',
        flow_run_id: null,
        transport: 'file',
        preview: 'Intermediate reasoning',
        available_sections: ['parameters', 'raw']
      }
    ],
    matches: [],
    previous_cursor: null,
    next_cursor: null,
    search_sections: ['overview', 'parameters', 'result'],
    integrity: 'complete'
  });
  const loadConversationClientTrajectorySection = vi
    .fn()
    .mockImplementation((_canonical, stepId, section) =>
      Promise.resolve({
        request_id: 'session-1',
        evidence_scope: 'step',
        step_id: stepId,
        section,
        items: [
          {
            sequence: 2,
            value:
              section === 'parameters'
                ? 'tool input from source'
                : { original: 'source event' }
          }
        ],
        next_cursor: null
      })
    );
  const loadRecordClientTrajectory = vi.fn();
  const loader: ConversationLogTraceLoader = {
    sourceKind: 'imported',
    queryConversationClientTrajectory,
    loadConversationClientTrajectorySection,
    loadRecordClientTrajectory
  };
  render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <ProviderTrajectory
        runId="record-1"
        logConversationId="canonical-1"
        loader={loader}
      />
    </QueryClientProvider>
  );
  expect(queryConversationClientTrajectory).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: '总轨迹' }));
  const call = await screen.findByRole('button', {
    name: '工具调用 · exec_command'
  });
  expect(queryConversationClientTrajectory).toHaveBeenCalledWith(
    'canonical-1',
    expect.objectContaining({ focus_record_id: 'record-1', cursor: undefined })
  );
  expect(loadRecordClientTrajectory).not.toHaveBeenCalled();
  expect(loadConversationClientTrajectorySection).not.toHaveBeenCalled();
  expect(screen.queryByText('内部调用')).not.toBeInTheDocument();
  fireEvent.click(call);
  expect(await screen.findByText('tool input from source')).toBeInTheDocument();
  await waitFor(() =>
    expect(loadConversationClientTrajectorySection).toHaveBeenCalledWith(
      'canonical-1',
      'call-1',
      'parameters',
      undefined
    )
  );
  expect(screen.getByText('responses · file')).toBeInTheDocument();
  expect(
    loadConversationClientTrajectorySection.mock.calls.some(
      (args) => args[2] === 'raw'
    )
  ).toBe(false);
  fireEvent.click(screen.getByRole('tab', { name: '原始协议' }));
  await waitFor(() =>
    expect(loadConversationClientTrajectorySection).toHaveBeenCalledWith(
      'canonical-1',
      'call-1',
      'raw',
      undefined
    )
  );
  expect(
    screen.queryByRole('button', { name: '打开内部调用' })
  ).not.toBeInTheDocument();
});

test('canonical trajectory preserves equal-sequence steps and follows opaque cursors without ordering or parsing them', async () => {
  const before = 's1:9:018f0000-0000-7000-8000-000000000001';
  const equalSequence = 's1:10:018f0000-0000-7000-8000-000000000002';
  const step = (
    id: string,
    sequence: number,
    name: string
  ): ConversationClientTrajectoryStep => ({
    ...tool,
    id,
    sequence,
    name,
    record_id: 'record-equal',
    log_conversation_id: 'canonical-equal',
    parent_task_run_id: null,
    is_subagent: false,
    source_kind: 'imported',
    source_id: 'source-1',
    source_client: 'codex',
    source_session_id: 'session-1',
    source_task_id: 'task-equal',
    flow_run_id: null,
    transport: 'file',
    parent_id: null,
    available_sections: ['parameters']
  });
  const queryConversationClientTrajectory = vi
    .fn()
    .mockImplementation(
      (_canonical: string, query: ConversationTrajectoryQuery) => {
        const base = {
          log_conversation_id: 'canonical-equal',
          matches: [],
          previous_cursor: null,
          search_sections: ['overview', 'parameters', 'result'],
          integrity: 'complete'
        };
        if (query.cursor === undefined)
          return Promise.resolve({
            ...base,
            items: [step('step-before', 9, 'previous')],
            next_cursor: before
          });
        if (query.cursor === before)
          return Promise.resolve({
            ...base,
            items: [step('step-first', 10, 'same-sequence-first')],
            next_cursor: equalSequence
          });
        if (query.cursor === equalSequence)
          return Promise.resolve({
            ...base,
            items: [step('step-second', 10, 'same-sequence-second')],
            next_cursor: null
          });
        throw new Error('Cursor was changed by consumer');
      }
    );
  const loadClientTrajectory = vi.fn();
  const loadRecordClientTrajectory = vi.fn();
  const loadConversationClientTrajectorySection = vi.fn().mockResolvedValue({
    request_id: root.id,
    evidence_scope: 'step',
    step_id: 'step-second',
    section: 'parameters',
    items: [{ sequence: 10, value: 'second same-sequence body' }],
    next_cursor: null
  });
  const loader: ConversationLogTraceLoader = {
    sourceKind: 'imported',
    queryConversationClientTrajectory,
    loadClientTrajectory,
    loadRecordClientTrajectory,
    loadConversationClientTrajectorySection
  };
  render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <ProviderTrajectory
        runId="record-equal"
        logConversationId="canonical-equal"
        loader={loader}
      />
    </QueryClientProvider>
  );
  fireEvent.click(screen.getByRole('button', { name: '总轨迹' }));
  await screen.findByRole('button', { name: '工具调用 · previous' });
  expect(queryConversationClientTrajectory).toHaveBeenCalledTimes(1);
  expect(
    screen.queryByRole('button', { name: '加载更新事件' })
  ).not.toBeInTheDocument();
  expect(
    await screen.findByRole('button', {
      name: '工具调用 · same-sequence-first'
    })
  ).toBeInTheDocument();
  expect(
    screen.queryByRole('button', { name: '加载更新事件' })
  ).not.toBeInTheDocument();
  const second = await screen.findByRole('button', {
    name: '工具调用 · same-sequence-second'
  });
  expect(queryConversationClientTrajectory).toHaveBeenCalledWith(
    'canonical-equal',
    expect.objectContaining({ focus_record_id: 'record-equal', cursor: before })
  );
  expect(queryConversationClientTrajectory).toHaveBeenCalledWith(
    'canonical-equal',
    expect.objectContaining({
      focus_record_id: 'record-equal',
      cursor: equalSequence
    })
  );
  expect(queryConversationClientTrajectory).toHaveBeenCalledTimes(3);
  expect(loadClientTrajectory).not.toHaveBeenCalled();
  expect(loadRecordClientTrajectory).not.toHaveBeenCalled();
  expect(loadConversationClientTrajectorySection).not.toHaveBeenCalled();
  fireEvent.click(second);
  expect(
    await screen.findByText('second same-sequence body')
  ).toBeInTheDocument();
  expect(loadConversationClientTrajectorySection).toHaveBeenCalledWith(
    'canonical-equal',
    'step-second',
    'parameters',
    undefined
  );
});
