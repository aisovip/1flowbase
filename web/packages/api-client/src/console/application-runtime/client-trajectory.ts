import { apiFetch } from '../../transport';
export interface ClientTrajectoryOptions {
  request_id?: string;
  focus_step_id?: string;
  index_only?: boolean;
  category?: string;
  keyword?: string;
  from?: string;
  to?: string;
}
export interface ClientTrajectoryStep {
  id: string;
  request_id: string;
  sequence: number;
  created_at: string;
  category: string;
  name: string;
  namespace: string | null;
  preview: string;
  parameters_preview: string | null;
  result_preview: string | null;
  status: string;
  origin: string;
  protocol: string;
  transport: 'http' | 'websocket' | 'file';
  flow_run_id: string | null;
  node_run_id: string | null;
  parent_id: string | null;
  call_id: string | null;
  item_id: string | null;
  response_id: string | null;
  turn_id: string | null;
  related_step_id: string | null;
  available_sections: string[];
}
export interface ClientTrajectoryIndexItem {
  protocol: string;
  preview?: string;
  id: string;
  sequence: number;
  created_at: string;
  category: string;
  request_id: string;
  flow_run_id: string | null;
  node_run_id: string | null;
}
export interface TrajectoryOverview<T> {
  request_roots?: ClientTrajectoryIndexItem[];
  total_count: number;
  category_counts: Record<string, number>;
  time_start: string | null;
  time_end: string | null;
  items: T[];
}
export interface ClientTrajectoryPage {
  previous_cursor?: number | null;
  overview?: TrajectoryOverview<ClientTrajectoryIndexItem>;
  items: ClientTrajectoryStep[];
  next_cursor: number | null;
  integrity: string;
}
export interface ClientTrajectorySection {
  request_id: string;
  evidence_scope: 'capture' | 'step';
  step_id: string;
  section: string;
  items: Array<{ sequence: number; value: unknown }>;
  next_cursor: number | null;
}
export function getConsoleClientTrajectory(
  applicationId: string,
  runId: string,
  nodeRunId?: string,
  cursor?: number,
  baseUrl?: string,
  options?: ClientTrajectoryOptions
) {
  const query = new URLSearchParams({
    limit: cursor === undefined ? '50' : '100'
  });
  if (options?.request_id) query.set('request_id', options.request_id);
  if (options?.focus_step_id && cursor === undefined)
    query.set('focus_step_id', options.focus_step_id);
  for (const key of [
    'index_only',
    'category',
    'keyword',
    'from',
    'to'
  ] as const) {
    const value = options?.[key];
    if (value !== undefined) query.set(key, String(value));
  }
  if (nodeRunId) query.set('node_run_id', nodeRunId);
  if (cursor !== undefined) query.set('cursor', String(cursor));
  return apiFetch<ClientTrajectoryPage>({
    path: `/api/console/applications/${applicationId}/logs/runs/${runId}/client-trajectory?${query}`,
    baseUrl
  });
}
export function getConsoleClientTrajectorySection(
  applicationId: string,
  runId: string,
  stepId: string,
  section: string,
  nodeRunId?: string,
  cursor?: number,
  baseUrl?: string
) {
  const query = new URLSearchParams({ limit: '8', section });
  if (nodeRunId) query.set('node_run_id', nodeRunId);
  if (cursor !== undefined) query.set('cursor', String(cursor));
  return apiFetch<ClientTrajectorySection>({
    path: `/api/console/applications/${applicationId}/logs/runs/${runId}/client-trajectory/${stepId}?${query}`,
    baseUrl
  });
}
