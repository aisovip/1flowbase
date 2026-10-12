import { apiFetch } from '../../transport';
import type {
  ClientTrajectoryStep,
  ClientTrajectoryIndexItem,
  TrajectoryOverview,
  ClientTrajectorySection
} from './client-trajectory';
export interface ConversationTrajectoryQuery {
  filter?: Record<string, unknown>;
  cursor?: string;
  limit?: number;
  keyword?: string;
  search_sections?: string[];
  focus_record_id?: string;
  focus_step_id?: string;
  index_only?: boolean;
}
export interface ConversationClientTrajectoryStep extends ClientTrajectoryStep {
  record_id: string;
  log_conversation_id: string;
  parent_task_run_id: string | null;
  is_subagent: boolean;
  source_kind: string;
  source_id: string | null;
  source_client: string | null;
  source_session_id: string | null;
  source_task_id: string | null;
}
export interface RecordTrajectoryMatch {
  step_id: string;
  section: string;
  sequence: number;
  snippet: string;
}
export interface ConversationTrajectoryIndexItem extends ClientTrajectoryIndexItem {
  record_id: string;
  log_conversation_id: string;
  is_subagent: boolean;
}
export interface ConversationClientTrajectoryQueryPage {
  overview?: TrajectoryOverview<ConversationTrajectoryIndexItem>;
  log_conversation_id: string;
  items: ConversationClientTrajectoryStep[];
  matches: RecordTrajectoryMatch[];
  previous_cursor: string | null;
  next_cursor: string | null;
  search_sections: string[];
  integrity: string;
}
const trajectoryPath = (applicationId: string, logConversationId: string) =>
  `/api/console/applications/${applicationId}/logs/conversations/${logConversationId}/client-trajectory`;
export function queryConsoleConversationClientTrajectory(
  applicationId: string,
  logConversationId: string,
  input: ConversationTrajectoryQuery = {},
  baseUrl?: string
) {
  return apiFetch<ConversationClientTrajectoryQueryPage>({
    path: `${trajectoryPath(applicationId, logConversationId)}/query`,
    method: 'POST',
    body: input,
    baseUrl
  });
}
export function getConsoleConversationClientTrajectorySection(
  applicationId: string,
  logConversationId: string,
  stepId: string,
  section: string,
  cursor?: number,
  baseUrl?: string
) {
  const query = new URLSearchParams({ limit: '8', section });
  if (cursor !== undefined) query.set('cursor', String(cursor));
  return apiFetch<ClientTrajectorySection>({
    path: `${trajectoryPath(applicationId, logConversationId)}/${encodeURIComponent(stepId)}?${query}`,
    baseUrl
  });
}
