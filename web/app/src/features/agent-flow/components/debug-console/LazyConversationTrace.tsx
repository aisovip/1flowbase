import { useCallback, useContext, useMemo, useReducer, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Alert, Button, Empty, Spin, Typography } from 'antd';
import { ClientTrajectoryWorkspace } from './trajectory/client/ClientTrajectoryWorkspace';
import { TraceActivityDetailContext } from './trajectory/activities/activity-model';
import { ProviderTrajectory } from './trajectory/ProviderTrajectory';
import { DebugWorkflowNodeItem } from './conversation/DebugWorkflowNodeRow';
import { DebugWorkflowNodeDetailContent } from './conversation/LlmToolTraceTree';
import { nodeDisplayName } from './conversation/debug-workflow-trace-utils';
import {
  appendTraceChildrenPage,
  createInitialLazyTraceChildrenState,
  lazyTraceChildrenReducer
} from './lazy-trace-children-state';
import type { RuntimeDebugArtifactBatchLoader } from '../detail/last-run/runtime-debug-payload';
import { i18nText } from '../../../../shared/i18n/text';
import type {
  ConversationLogTraceLoader,
  ConversationLogTraceNodeChildren,
  ConversationLogTraceNodeChildrenPageInfo,
  ConversationLogTraceNodeSummary,
  ConversationLogTraceProjectionStatus
} from './conversation-log-trace-model';
import {
  findNodeRunDetailRefId,
  isTraceGroupNode,
  isToolModeTraceNode,
  mapTraceContentToTraceItem,
  mapTraceSummaryToTraceItem,
  traceItemWithToolMode,
  traceProjectionStatusSucceeded,
  toolModeFromTraceNodes
} from './conversation-log-trace-model';
const CONVERSATION_LOG_QUERY_STALE_TIME_MS = 60_000;
const initialLazyTraceChildrenState = createInitialLazyTraceChildrenState<
  ConversationLogTraceNodeSummary,
  ConversationLogTraceNodeChildrenPageInfo,
  ConversationLogTraceProjectionStatus
>();

function traceProjectionStatusMessage(
  status: ConversationLogTraceProjectionStatus
) {
  switch (status.projection_status) {
    case 'pending':
      return i18nText('agentFlow', 'auto.trace_projection_pending');
    case 'running':
      return i18nText('agentFlow', 'auto.trace_projection_running');
    case 'failed':
      return i18nText('agentFlow', 'auto.trace_projection_failed');
    case 'stale':
      return i18nText('agentFlow', 'auto.trace_projection_stale');
    case 'partial':
      return i18nText('agentFlow', 'auto.trace_projection_partial');
    case 'succeeded':
      return i18nText('agentFlow', 'auto.trace_projection_succeeded');
  }
}

export function TraceProjectionStatusNotice({
  status
}: {
  status: ConversationLogTraceProjectionStatus;
}) {
  return (
    <Alert
      className="agent-flow-editor__conversation-log-projection-status"
      title={traceProjectionStatusMessage(status)}
      showIcon
      type={status.projection_status === 'failed' ? 'error' : 'info'}
    />
  );
}

export function LazyConversationTrace({
  defaultToolsExpanded,
  onLoadArtifact,
  onLoadArtifacts,
  runId,
  traceLoader
}: {
  defaultToolsExpanded: boolean;
  onLoadArtifact?: (artifactRef: string) => Promise<unknown>;
  onLoadArtifacts?: RuntimeDebugArtifactBatchLoader;
  runId: string;
  traceLoader: ConversationLogTraceLoader;
}) {
  const [rootPages, setRootPages] = useState<
    ConversationLogTraceNodeChildren[]
  >([]);
  const [loadingMore, setLoadingMore] = useState(false);
  const [loadFailed, setLoadFailed] = useState(false);
  const traceTreeQuery = useQuery({
    queryKey: [
      'conversation-log-trace-tree',
      traceLoader.sourceKind ?? 'native',
      runId
    ],
    queryFn: () => traceLoader.loadTree!(runId),
    refetchOnWindowFocus: false,
    refetchOnMount: 'always',
    refetchInterval: (query) => {
      const status = query.state.data?.projection_status?.projection_status;
      return status === 'pending' || status === 'running' ? 1000 : false;
    },
    staleTime: CONVERSATION_LOG_QUERY_STALE_TIME_MS
  });
  const rootPageInfo =
    rootPages.at(-1)?.page_info ?? traceTreeQuery.data?.page_info;
  const loadMoreRoots = async () => {
    if (!rootPageInfo?.next_cursor || loadingMore) return;
    setLoadingMore(true);
    setLoadFailed(false);
    try {
      const page = await traceLoader.loadChildren!(
        runId,
        'root',
        rootPageInfo.next_cursor
      );
      setRootPages((pages) => [...pages, page]);
    } catch {
      setLoadFailed(true);
    } finally {
      setLoadingMore(false);
    }
  };

  if (traceTreeQuery.isError) {
    return (
      <Alert
        type="error"
        showIcon
        title={i18nText('agentFlow', 'auto.loading_failed')}
      />
    );
  }

  if (traceTreeQuery.isLoading) {
    return (
      <div className="agent-flow-editor__conversation-log-empty">
        <Spin />
      </div>
    );
  }

  const projectionStatus = traceTreeQuery.data?.projection_status;
  const nodes = rootPages.reduce(
    (items, page) => appendTraceChildrenPage(items, page.items),
    traceTreeQuery.data?.nodes ?? []
  );

  if (!traceProjectionStatusSucceeded(projectionStatus) && projectionStatus) {
    return (
      <div className="agent-flow-editor__conversation-log-trace">
        <TraceProjectionStatusNotice status={projectionStatus} />
      </div>
    );
  }

  if (nodes.length === 0) {
    return (
      <div className="agent-flow-editor__conversation-log-empty">
        <Empty
          description={i18nText('agentFlow', 'auto.tracking_record_yet')}
          image={Empty.PRESENTED_IMAGE_SIMPLE}
        />
      </div>
    );
  }

  return (
    <div className="agent-flow-editor__conversation-log-trace">
      <LazyTraceNodeList
        defaultToolsExpanded={defaultToolsExpanded}
        nodes={nodes}
        onLoadArtifact={onLoadArtifact}
        onLoadArtifacts={onLoadArtifacts}
        runId={runId}
        traceLoader={traceLoader}
      />
      {loadFailed ? (
        <Alert
          type="error"
          showIcon
          title={i18nText('agentFlow', 'auto.loading_failed')}
        />
      ) : null}
      {rootPageInfo?.has_more ? (
        <Button
          type="link"
          size="small"
          loading={loadingMore}
          onClick={() => void loadMoreRoots()}
        >
          {i18nText('agentFlow', 'auto.load_more_trace_children')}
        </Button>
      ) : null}
    </div>
  );
}

function LazyTraceNodeList({
  defaultToolsExpanded,
  nodes,
  onLoadArtifact,
  onLoadArtifacts,
  runId,
  traceLoader
}: {
  defaultToolsExpanded: boolean;
  nodes: ConversationLogTraceNodeSummary[];
  onLoadArtifact?: (artifactRef: string) => Promise<unknown>;
  onLoadArtifacts?: RuntimeDebugArtifactBatchLoader;
  runId: string;
  traceLoader: ConversationLogTraceLoader;
}) {
  return (
    <div
      aria-label={i18nText('agentFlow', 'auto.tracking_nodes')}
      className="agent-flow-editor__conversation-log-node-list"
    >
      {nodes.map((node) => (
        <LazyTraceNodeItem
          key={`${runId}:${node.trace_node_id}`}
          defaultToolsExpanded={defaultToolsExpanded}
          node={node}
          onLoadArtifact={onLoadArtifact}
          onLoadArtifacts={onLoadArtifacts}
          runId={runId}
          traceLoader={traceLoader}
        />
      ))}
    </div>
  );
}

function FlattenedToolModeTraceNodeChildren({
  defaultToolsExpanded,
  nodes,
  onLoadArtifact,
  onLoadArtifacts,
  runId,
  traceLoader
}: {
  defaultToolsExpanded: boolean;
  nodes: ConversationLogTraceNodeSummary[];
  onLoadArtifact?: (artifactRef: string) => Promise<unknown>;
  onLoadArtifacts?: RuntimeDebugArtifactBatchLoader;
  runId: string;
  traceLoader: ConversationLogTraceLoader;
}) {
  if (nodes.length === 0) {
    return null;
  }

  return (
    <>
      {nodes.map((node) => (
        <FlattenedToolModeTraceNodeChild
          key={node.trace_node_id}
          defaultToolsExpanded={defaultToolsExpanded}
          node={node}
          onLoadArtifact={onLoadArtifact}
          onLoadArtifacts={onLoadArtifacts}
          runId={runId}
          traceLoader={traceLoader}
        />
      ))}
    </>
  );
}

function FlattenedToolModeTraceNodeChild({
  defaultToolsExpanded,
  node,
  onLoadArtifact,
  onLoadArtifacts,
  runId,
  traceLoader
}: {
  defaultToolsExpanded: boolean;
  node: ConversationLogTraceNodeSummary;
  onLoadArtifact?: (artifactRef: string) => Promise<unknown>;
  onLoadArtifacts?: RuntimeDebugArtifactBatchLoader;
  runId: string;
  traceLoader: ConversationLogTraceLoader;
}) {
  const childrenQuery = useQuery({
    enabled: node.has_children,
    queryKey: [
      'conversation-log-trace-node-children',
      runId,
      node.trace_node_id
    ],
    queryFn: () =>
      traceLoader.loadChildren!(runId, node.trace_node_id, undefined),
    refetchOnWindowFocus: false,
    staleTime: CONVERSATION_LOG_QUERY_STALE_TIME_MS
  });
  const childProjectionStatus = childrenQuery.data?.projection_status;
  const childNodes = childrenQuery.data?.items ?? [];

  if (!node.has_children) {
    return null;
  }

  return (
    <>
      {childrenQuery.isLoading ? <Spin /> : null}
      {childrenQuery.isError ? (
        <Alert
          title={i18nText('agentFlow', 'auto.loading_failed')}
          showIcon
          type="error"
        />
      ) : null}
      {childProjectionStatus &&
      !traceProjectionStatusSucceeded(childProjectionStatus) ? (
        <TraceProjectionStatusNotice status={childProjectionStatus} />
      ) : null}
      {childNodes.length > 0 ? (
        <LazyTraceNodeList
          defaultToolsExpanded={defaultToolsExpanded}
          nodes={childNodes}
          onLoadArtifact={onLoadArtifact}
          onLoadArtifacts={onLoadArtifacts}
          runId={runId}
          traceLoader={traceLoader}
        />
      ) : null}
    </>
  );
}

export function LazyTraceNodeItem({
  detailOnly = false,
  initiallyExpanded = false,
  defaultToolsExpanded,
  node,
  onLoadArtifact,
  onLoadArtifacts,
  runId,
  traceLoader
}: {
  defaultToolsExpanded: boolean;
  initiallyExpanded?: boolean;
  detailOnly?: boolean;
  node: ConversationLogTraceNodeSummary;
  onLoadArtifact?: (artifactRef: string) => Promise<unknown>;
  onLoadArtifacts?: RuntimeDebugArtifactBatchLoader;
  runId: string;
  traceLoader: ConversationLogTraceLoader;
}) {
  const activityDetail = useContext(TraceActivityDetailContext);
  const isGroupNode = isTraceGroupNode(node);
  const hasTrajectory = node.node_type === 'llm' && Boolean(node.node_run_id);
  const [expanded, setExpanded] = useState(initiallyExpanded);
  const [childrenState, dispatchChildrenState] = useReducer(
    lazyTraceChildrenReducer,
    initialLazyTraceChildrenState
  );
  const fallbackItem = useMemo(() => mapTraceSummaryToTraceItem(node), [node]);
  const contentQuery = useQuery({
    enabled:
      expanded &&
      node.has_content &&
      !isGroupNode &&
      !node.record_id &&
      Boolean(traceLoader.loadContent),
    queryKey: [
      'conversation-log-trace-node-content',
      runId,
      node.trace_node_id
    ],
    queryFn: () => traceLoader.loadContent!(runId, node.trace_node_id),
    refetchOnWindowFocus: false,
    staleTime: CONVERSATION_LOG_QUERY_STALE_TIME_MS
  });
  const nodeRunDetailRefId = useMemo(
    () => findNodeRunDetailRefId(contentQuery.data),
    [contentQuery.data]
  );
  const loadNodeRunSection =
    nodeRunDetailRefId && traceLoader.loadDetail
      ? async (
          section: 'input_payload' | 'debug_payload' | 'output_payload'
        ) => {
          const result = await traceLoader.loadDetail!(
            runId,
            node.trace_node_id,
            nodeRunDetailRefId,
            section
          );
          const nodeRun = result.payload.node_run as
            | Record<string, unknown>
            | undefined;
          return nodeRun?.[section];
        }
      : undefined;
  const childrenQuery = useQuery({
    enabled: expanded && node.has_children && !detailOnly,
    queryKey: [
      'conversation-log-trace-node-children',
      runId,
      node.trace_node_id
    ],
    queryFn: () =>
      traceLoader.loadChildren!(runId, node.trace_node_id, undefined),
    refetchOnWindowFocus: false,
    staleTime: CONVERSATION_LOG_QUERY_STALE_TIME_MS
  });
  const childNodes = useMemo(
    () =>
      appendTraceChildrenPage(
        childrenQuery.data?.items ?? [],
        childrenState.additionalNodes
      ),
    [childrenState.additionalNodes, childrenQuery.data?.items]
  );
  const childPageInfo =
    childrenState.pageInfo ?? childrenQuery.data?.page_info ?? null;
  const childProjectionStatus =
    childrenState.projectionStatus ?? childrenQuery.data?.projection_status;
  const loadMoreTraceChildren = useCallback(async () => {
    const cursor = childPageInfo?.next_cursor;
    if (!cursor || childrenState.loadingMore) {
      return;
    }

    dispatchChildrenState({ type: 'load_more_started' });
    try {
      const nextPage = await traceLoader.loadChildren!(
        runId,
        node.trace_node_id,
        cursor
      );
      dispatchChildrenState({ type: 'load_more_succeeded', page: nextPage });
    } catch {
      dispatchChildrenState({ type: 'load_more_failed' });
    }
  }, [
    childPageInfo?.next_cursor,
    childrenState.loadingMore,
    node.trace_node_id,
    runId,
    traceLoader
  ]);
  const toolModeNodes = useMemo(
    () =>
      node.node_kind === 'tool_callback'
        ? childNodes.filter(isToolModeTraceNode)
        : [],
    [childNodes, node.node_kind]
  );
  const visibleChildNodes = useMemo(
    () =>
      node.node_kind === 'tool_callback'
        ? childNodes.filter((childNode) => !isToolModeTraceNode(childNode))
        : childNodes,
    [childNodes, node.node_kind]
  );
  const item = useMemo(
    () =>
      traceItemWithToolMode(
        mapTraceContentToTraceItem(fallbackItem, contentQuery.data),
        toolModeFromTraceNodes(childNodes)
      ),
    [childNodes, contentQuery.data, fallbackItem]
  );
  const contentProjectionStatus = contentQuery.data?.projection_status;
  const contentLoading = contentQuery.isLoading;
  const loadToolCallbackDetail = traceLoader.loadToolCallbackDetail;
  const childNodesBeforePayload =
    !detailOnly &&
    (visibleChildNodes.length > 0 || toolModeNodes.length > 0) ? (
      <>
        <FlattenedToolModeTraceNodeChildren
          defaultToolsExpanded={defaultToolsExpanded}
          nodes={toolModeNodes}
          onLoadArtifact={onLoadArtifact}
          onLoadArtifacts={onLoadArtifacts}
          runId={runId}
          traceLoader={traceLoader}
        />
        {visibleChildNodes.length > 0 ? (
          <LazyTraceNodeList
            defaultToolsExpanded={defaultToolsExpanded}
            nodes={visibleChildNodes}
            onLoadArtifact={onLoadArtifact}
            onLoadArtifacts={onLoadArtifacts}
            runId={runId}
            traceLoader={traceLoader}
          />
        ) : null}
      </>
    ) : null;
  const childLoadStatusContent = (
    <>
      {childrenQuery.isLoading ? <Spin /> : null}
      {childProjectionStatus &&
      !traceProjectionStatusSucceeded(childProjectionStatus) ? (
        <TraceProjectionStatusNotice status={childProjectionStatus} />
      ) : null}
      {childrenState.loadMoreFailed ? (
        <Alert
          title={i18nText('agentFlow', 'auto.loading_failed')}
          showIcon
          type="error"
        />
      ) : null}
    </>
  );
  const loadMoreChildrenButton = childPageInfo?.has_more ? (
    <Button
      className="agent-flow-editor__conversation-log-load-more"
      loading={childrenState.loadingMore}
      size="small"
      type="link"
      onClick={() => {
        void loadMoreTraceChildren();
      }}
    >
      {i18nText('agentFlow', 'auto.load_more_trace_children')}
    </Button>
  ) : null;

  const detail = node.record_id ? (
    <section className="agent-flow-editor__conversation-log-node-detail">
      {expanded && node.has_content ? (
        <ClientTrajectoryWorkspace
          runId={node.record_id}
          loader={traceLoader}
        />
      ) : null}
      {childLoadStatusContent}
      {childrenQuery.isError ? (
        <Alert
          type="error"
          showIcon
          title={i18nText('agentFlow', 'auto.loading_failed')}
        />
      ) : null}
      {childNodesBeforePayload}
      {loadMoreChildrenButton}
    </section>
  ) : isGroupNode ? (
    <div className="agent-flow-editor__conversation-log-node-group">
      {childLoadStatusContent}
      {childNodesBeforePayload}
      {loadMoreChildrenButton}
    </div>
  ) : (
    <section
      aria-label={i18nText('agentFlow', 'auto.node_details_alt', {
        value1: nodeDisplayName(fallbackItem)
      })}
      className="agent-flow-editor__conversation-log-node-detail"
    >
      {contentLoading ? (
        <Spin />
      ) : contentProjectionStatus &&
        !traceProjectionStatusSucceeded(contentProjectionStatus) ? (
        <TraceProjectionStatusNotice status={contentProjectionStatus} />
      ) : (
        <div className="agent-flow-editor__conversation-log-json-list">
          <DebugWorkflowNodeDetailContent
            onLoadSection={loadNodeRunSection}
            beforePayloadContent={childNodesBeforePayload}
            processAction={
              !activityDetail && hasTrajectory && node.node_run_id ? (
                <ProviderTrajectory
                  runId={node.source_flow_run_id ?? node.flow_run_id ?? runId}
                  nodeRunId={node.node_run_id}
                  loader={traceLoader}
                />
              ) : undefined
            }
            defaultToolsExpanded={defaultToolsExpanded}
            item={item}
            onLoadArtifact={onLoadArtifact}
            onLoadArtifacts={onLoadArtifacts}
            onLoadToolCallbackDetail={
              loadToolCallbackDetail
                ? (toolCallId) =>
                    loadToolCallbackDetail(
                      runId,
                      node.trace_node_id,
                      toolCallId
                    )
                : undefined
            }
          />
        </div>
      )}
      {childLoadStatusContent}
      {loadMoreChildrenButton}
    </section>
  );
  if (detailOnly) return detail;
  return (
    <>
      {node.record_id && node.child_count ? (
        <Typography.Text type="secondary">
          {i18nText('applications', 'auto.subagents', {
            count: node.child_count
          })}
        </Typography.Text>
      ) : null}
      <DebugWorkflowNodeItem
        expanded={expanded}
        item={item}
        onToggle={() => setExpanded((current) => !current)}
      >
        {detail}
      </DebugWorkflowNodeItem>
    </>
  );
}
