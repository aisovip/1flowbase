import RightOutlined from '@ant-design/icons/es/icons/RightOutlined';
import type { ConsoleApplicationLogRecordOverview } from '@1flowbase/api-client';
import { useInfiniteQuery } from '@tanstack/react-query';
import { Alert, Button, Flex, Spin, Typography } from 'antd';
import { useMemo, useState } from 'react';
import type { AgentFlowDebugMessage } from '../../../../agent-flow/api/runtime';
import { DebugAssistantMessage } from '../../../../agent-flow/components/debug-console/conversation/DebugAssistantMessage';
import { DebugMarkdownContent } from '../../../../agent-flow/components/debug-console/conversation/DebugMarkdownContent';
import { ProviderTrajectory } from '../../../../agent-flow/components/debug-console/trajectory/ProviderTrajectory';
import { i18nText } from '../../../../../shared/i18n/text';
import { fetchApplicationLogRecordConversationMessages } from '../../../api/trajectory';
import {
  buildApplicationRunTraceMessage,
  createApplicationLogTraceLoader
} from '../application-log-trace-loader';
import './application-log-subagents.css';

const refreshableStatuses = new Set([
  'queued', 'running', 'paused', 'waiting_callback', 'waiting_human'
]);

interface SubagentsProps {
  applicationId: string;
  record: ConsoleApplicationLogRecordOverview;
  onOpenMessageLog?: (message: AgentFlowDebugMessage) => void;
}

function ChildRecord({ applicationId, record, onOpenMessageLog }: SubagentsProps) {
  const traceLoader = useMemo(
    () => createApplicationLogTraceLoader(applicationId, 'imported'),
    [applicationId]
  );
  const canOpenDetail = record.available_views.includes('client_trajectory');
  return (
    <section className="application-log-subagents__record" data-record-id={record.record_id}>
      <Flex align="center" gap={8} wrap>
        <Typography.Text strong>{record.title || record.record_id}</Typography.Text>
        <Typography.Text type="secondary">{record.status}</Typography.Text>
        {canOpenDetail ? (
          <>
            <ProviderTrajectory runId={record.record_id} loader={traceLoader} buttonType="default" />
            {onOpenMessageLog ? (
              <Button size="small" onClick={() => onOpenMessageLog(
                buildApplicationRunTraceMessage(record.record_id)
              )}>
                {i18nText('agentFlow', 'auto.view_conversation_log')}
              </Button>
            ) : null}
          </>
        ) : null}
      </Flex>
      {record.messages.map((item) => {
        const message: AgentFlowDebugMessage = {
          id: 'record-' + record.record_id + '-' + item.sequence,
          role: item.role as AgentFlowDebugMessage['role'],
          content: item.content,
          status: record.status === 'running' || record.status === 'queued'
            ? 'running' : record.status === 'failed' ? 'failed'
            : record.status === 'cancelled' ? 'cancelled'
            : record.status === 'waiting_human' ? 'waiting_human'
            : record.status === 'waiting_callback' ? 'waiting_callback' : 'completed',
          runId: record.native_run_id,
          detailRunId: record.record_id,
          canOpenDetail: false,
          rawOutput: null,
          traceSummary: []
        };
        return (
          <div key={item.sequence} className="application-log-subagents__message" data-message-role={item.role}>
            {item.role === 'assistant' ? (
              <DebugAssistantMessage message={message} />
            ) : item.role === 'user' ? (
              <Typography.Paragraph>{item.content}</Typography.Paragraph>
            ) : (
              <DebugMarkdownContent content={item.content} />
            )}
          </div>
        );
      })}
      {record.messages.length === 0 ? (
        <Typography.Text type="secondary">
          {i18nText('applications', 'auto.subagent_no_messages')}
        </Typography.Text>
      ) : null}
      {record.child_task_count > 0 ? (
        <ApplicationLogSubagents applicationId={applicationId} record={record}
          onOpenMessageLog={onOpenMessageLog} />
      ) : null}
    </section>
  );
}

export function ApplicationLogSubagents({ applicationId, record, onOpenMessageLog }: SubagentsProps) {
  const [expanded, setExpanded] = useState(false);
  const childrenQuery = useInfiniteQuery({
    queryKey: ['application-log-record-conversation', applicationId, record.record_id, 'children'],
    enabled: expanded,
    initialPageParam: {} as { before?: string; after?: string },
    queryFn: ({ pageParam }) => fetchApplicationLogRecordConversationMessages(
      applicationId, record.record_id, { ...pageParam, scope: 'children', limit: 5 }
    ),
    getNextPageParam: (page) => page.page.has_before && page.page.before_cursor
      ? { before: page.page.before_cursor } : undefined,
    getPreviousPageParam: (page) => page.page.has_after && page.page.after_cursor
      ? { after: page.page.after_cursor } : undefined,
    refetchInterval: (query) => expanded && (
      refreshableStatuses.has(record.status) ||
      query.state.data?.pages.some((page) => page.items.some(
        (child) => refreshableStatuses.has(child.status)
      ))
    ) ? 1000 : false,
    refetchOnWindowFocus: false
  });
  const children = useMemo(() => {
    const records = new Map<string, ConsoleApplicationLogRecordOverview>();
    for (const page of [...(childrenQuery.data?.pages ?? [])].reverse()) {
      for (const child of page.items) records.set(child.record_id, child);
    }
    return [...records.values()];
  }, [childrenQuery.data]);

  return (
    <div className="application-log-subagents" data-parent-record-id={record.record_id}>
      {record.messages.length === 0 ? (
        <Flex align="center" gap={8} wrap>
          <Typography.Text strong>{record.title || record.record_id}</Typography.Text>
          <Typography.Text type="secondary">{record.status}</Typography.Text>
        </Flex>
      ) : null}
      <Button size="small" type="text" aria-expanded={expanded}
        icon={<RightOutlined aria-hidden="true" rotate={expanded ? 90 : 0} />}
        aria-controls={'subagents-' + record.record_id}
        onClick={() => setExpanded((current) => !current)}>
        {i18nText('applications', 'auto.subagents', { count: record.child_task_count })}
      </Button>
      {expanded ? (
        <div id={'subagents-' + record.record_id} className="application-log-subagents__children">
          {childrenQuery.isFetching && !childrenQuery.data ? <Spin size="small" /> : null}
          {childrenQuery.isError ? (
            <Alert type="error" title={i18nText('agentFlow', 'auto.loading_failed')}
              action={<Button size="small" onClick={() => {
                if (childrenQuery.isFetchNextPageError) void childrenQuery.fetchNextPage();
                else if (childrenQuery.isFetchPreviousPageError) void childrenQuery.fetchPreviousPage();
                else void childrenQuery.refetch();
              }}>{i18nText('agentFlow', 'auto.retry')}</Button>} />
          ) : null}
          {childrenQuery.hasNextPage ? (
            <Button size="small" loading={childrenQuery.isFetchingNextPage}
              onClick={() => void childrenQuery.fetchNextPage()}>
              {i18nText('applications', 'auto.subagents_older')}
            </Button>
          ) : null}
          {children.map((child) => (
            <ChildRecord key={child.record_id} applicationId={applicationId}
              record={child} onOpenMessageLog={onOpenMessageLog} />
          ))}
          {childrenQuery.isSuccess && children.length === 0 ? (
            <Typography.Text type="secondary">
              {i18nText('applications', 'auto.subagents_empty')}
            </Typography.Text>
          ) : null}
          {childrenQuery.hasPreviousPage ? (
            <Button size="small" loading={childrenQuery.isFetchingPreviousPage}
              onClick={() => void childrenQuery.fetchPreviousPage()}>
              {i18nText('applications', 'auto.subagents_newer')}
            </Button>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
