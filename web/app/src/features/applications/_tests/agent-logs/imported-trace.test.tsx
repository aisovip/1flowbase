import '../../../../test/fixtures/rc-util-unique-ids';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { App } from 'antd';
import { beforeEach, expect, test, vi } from 'vitest';
import type {
  ClientTrajectoryStep,
  ApplicationLogRecordTraceNode
} from '@1flowbase/api-client';
import type { ConversationLogTraceLoader } from '../../../agent-flow/components/debug-console/conversation-log-trace-model';
import { ConversationLogPanel } from '../../../agent-flow/components/debug-console/ConversationLogPanel';
import { ProviderTrajectory } from '../../../agent-flow/components/debug-console/trajectory/ProviderTrajectory';
import { buildApplicationRunTraceMessage } from '../../components/logs/application-log-trace-loader';
import { appI18n } from '../../../../shared/i18n/app-i18n';

beforeEach(async () => {
  await appI18n.changeLanguage('zh_Hans');
});
const node = (
  record_id: string,
  node_alias: string,
  parent_trace_node_id: string | null,
  has_children: boolean
): ApplicationLogRecordTraceNode => ({
  trace_node_id: record_id,
  record_id,
  parent_trace_node_id,
  node_kind: parent_trace_node_id ? 'child_task' : 'task_round',
  node_alias,
  status: 'succeeded',
  started_at: '2026-10-12T00:00:00Z',
  finished_at: null,
  duration_ms: null,
  metrics_payload: {},
  has_children,
  child_count: has_children ? 1 : 0,
  has_content: true
});
const step: ClientTrajectoryStep = {
  id: 'tool-source-event',
  request_id: 'child-session',
  sequence: 49,
  created_at: '2026-10-12T00:00:00Z',
  category: 'tool_call',
  name: 'exec_command',
  namespace: null,
  preview: 'Source tool',
  parameters_preview: null,
  result_preview: null,
  status: 'succeeded',
  origin: 'emitted',
  protocol: 'responses',
  transport: 'file',
  flow_run_id: null,
  node_run_id: null,
  parent_id: null,
  call_id: 'source-call',
  item_id: null,
  response_id: null,
  turn_id: null,
  related_step_id: null,
  available_sections: ['parameters', 'raw']
};

test.each(['message trace', 'trajectory window'])(
  '%s uses the same lazy nested record tree and own source sections',
  async (entrypoint) => {
    const loadTree = vi.fn().mockResolvedValue({
      nodes: [node('old-main', 'Old main task', null, true)]
    });
    const loadChildren = vi
      .fn()
      .mockImplementation(async (_record, parent, cursor) => ({
        items:
          parent === 'old-main'
            ? cursor
              ? [
                  node(
                    'sibling-record',
                    'Next-page delegated task',
                    'old-main',
                    false
                  )
                ]
              : [node('child-record', 'Delegated task', 'old-main', true)]
            : [
                node(
                  'nested-record',
                  'Nested delegated task',
                  'child-record',
                  false
                )
              ],
        page_info: {
          has_more: parent === 'old-main' && !cursor,
          next_cursor:
            parent === 'old-main' && !cursor ? 'bound-child-cursor' : null,
          page_size: 20
        }
      }));
    const loadRecordClientTrajectory = vi
      .fn()
      .mockImplementation(async (record) => ({
        items: record === 'child-record' ? [step] : [],
        next_cursor: null,
        integrity: 'complete'
      }));
    const loadClientTrajectorySection = vi
      .fn()
      .mockImplementation(async (_record, step_id, section) => ({
        request_id: 'child-session',
        evidence_scope: 'step',
        step_id,
        section,
        items: [
          {
            sequence: 49,
            value:
              section === 'raw'
                ? 'Original child source event'
                : 'Child tool parameters'
          }
        ],
        next_cursor: null
      }));
    const loadContent = vi.fn();
    const loader: ConversationLogTraceLoader = {
      sourceKind: 'imported',
      loadTree,
      loadChildren,
      loadRecordClientTrajectory,
      loadClientTrajectorySection,
      loadContent
    };
    render(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <App>
          {entrypoint === 'message trace' ? (
            <ConversationLogPanel
              message={buildApplicationRunTraceMessage('old-main')}
              activeTab="trace"
              traceLoader={loader}
              onClose={vi.fn()}
            />
          ) : (
            <ProviderTrajectory runId="old-main" loader={loader} />
          )}
        </App>
      </QueryClientProvider>
    );
    if (entrypoint === 'trajectory window') {
      expect(loadTree).not.toHaveBeenCalled();
      fireEvent.click(screen.getByRole('button', { name: '总轨迹' }));
    }
    const root = await screen.findByRole('button', { name: /Old main task/ });
    expect(loadChildren).not.toHaveBeenCalled();
    expect(loadRecordClientTrajectory).not.toHaveBeenCalled();
    expect(screen.getByText('子代理（1）')).toBeInTheDocument();
    fireEvent.click(root);
    const child = await screen.findByRole('button', { name: /Delegated task/ });
    expect(loadChildren).toHaveBeenCalledWith(
      'old-main',
      'old-main',
      undefined
    );
    expect(
      loadChildren.mock.calls.some((call) => call[1] === 'child-record')
    ).toBe(false);
    expect(loadRecordClientTrajectory).toHaveBeenCalledWith(
      'old-main',
      undefined,
      undefined,
      undefined
    );
    fireEvent.click(child);
    await screen.findByRole('button', { name: /Nested delegated task/ });
    expect(loadChildren).toHaveBeenCalledWith(
      'old-main',
      'child-record',
      undefined
    );
    expect(loadRecordClientTrajectory).toHaveBeenCalledWith(
      'child-record',
      undefined,
      undefined,
      undefined
    );
    expect(
      loadRecordClientTrajectory.mock.calls.some(
        (call) => call[0] === 'nested-record'
      )
    ).toBe(false);
    expect(loadClientTrajectorySection).not.toHaveBeenCalled();
    fireEvent.click(
      await screen.findByRole('button', { name: '工具调用 · exec_command' })
    );
    expect(
      await screen.findByText('Child tool parameters')
    ).toBeInTheDocument();
    expect(
      loadClientTrajectorySection.mock.calls.some((call) => call[2] === 'raw')
    ).toBe(false);
    fireEvent.click(screen.getByRole('tab', { name: '原始协议' }));
    await waitFor(() =>
      expect(loadClientTrajectorySection).toHaveBeenCalledWith(
        'child-record',
        'tool-source-event',
        'raw',
        undefined,
        undefined
      )
    );
    expect(
      await screen.findByText('Original child source event')
    ).toBeInTheDocument();
    expect(
      loadChildren.mock.calls.some((call) => call[2] === 'bound-child-cursor')
    ).toBe(false);
    fireEvent.click(screen.getByRole('button', { name: '加载更多' }));
    await screen.findByRole('button', { name: /Next-page delegated task/ });
    expect(loadChildren).toHaveBeenCalledWith(
      'old-main',
      'old-main',
      'bound-child-cursor'
    );
    expect(
      screen.getByRole('button', { name: /Nested delegated task/ })
    ).toBeInTheDocument();
    expect(
      loadRecordClientTrajectory.mock.calls.some(
        (call) => call[0] === 'sibling-record'
      )
    ).toBe(false);
    expect(loadContent).not.toHaveBeenCalled();
    expect(screen.queryByText('内部调用')).not.toBeInTheDocument();
  }
);
