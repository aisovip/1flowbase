import { queryOptions } from '@tanstack/react-query';
import type { WorkflowTrajectoryOptions } from '@1flowbase/api-client';
import type { ConversationLogTraceLoader } from '../../conversation-log-trace-model';

export function workflowTrajectoryQueryOptions(
  runId: string,
  loader: ConversationLogTraceLoader,
  filters: WorkflowTrajectoryOptions
) {
  return queryOptions({
    queryKey: ['workflow-trajectory', runId, filters, 'initial-window'],
    queryFn: () =>
      loader.loadWorkflowTrajectory!(runId, undefined, {
        ...filters,
        index_only: false
      }),
    staleTime: 60_000,
    refetchOnWindowFocus: false
  });
}
