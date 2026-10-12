import {
  getConsoleApplicationLogRecordTraceTree,
  getConsoleApplicationLogRecordTraceChildren,
  type RecordClientTrajectoryCursor,
  type ApplicationLogRecordConversationQuery,
  getConsoleApplicationLogRecordConversationMessages,
  getConsoleApplicationLogRecord,
  getConsoleApplicationLogRecordClientTrajectory,
  getConsoleApplicationLogRecordClientTrajectorySection,
  queryConsoleConversationClientTrajectory,
  getConsoleConversationClientTrajectorySection,
  type ConversationTrajectoryQuery,
  getConsoleWorkflowTrajectory,
  getConsoleWorkflowTrajectoryBody,
  type WorkflowTrajectoryOptions,
  getConsoleClientTrajectory,
  getConsoleClientTrajectorySection,
  getConsoleRunPayload,
  getConsoleProviderTrajectoryBody,
  type ClientTrajectoryOptions,
  type ProviderTrajectoryView
} from '@1flowbase/api-client';
import { getApplicationsApiBaseUrl } from './applications';

export function fetchProviderTrajectoryBody(
  applicationId: string,
  runId: string,
  nodeRunId: string,
  eventId: string,
  cursor?: number,
  view: ProviderTrajectoryView = 'semantic'
) {
  return getConsoleProviderTrajectoryBody(
    applicationId,
    runId,
    nodeRunId,
    eventId,
    cursor,
    view,
    getApplicationsApiBaseUrl()
  );
}

export function fetchRunPayload(
  applicationId: string,
  runId: string,
  section: 'input_payload' | 'output_payload'
) {
  return getConsoleRunPayload(
    applicationId,
    runId,
    section,
    getApplicationsApiBaseUrl()
  );
}

export function fetchClientTrajectory(
  applicationId: string,
  runId: string,
  nodeRunId?: string,
  cursor?: number,
  options?: ClientTrajectoryOptions
) {
  return getConsoleClientTrajectory(
    applicationId,
    runId,
    nodeRunId,
    cursor,
    getApplicationsApiBaseUrl(),
    options
  );
}
export function fetchClientTrajectorySection(
  applicationId: string,
  runId: string,
  stepId: string,
  section: string,
  nodeRunId?: string,
  cursor?: number
) {
  return getConsoleClientTrajectorySection(
    applicationId,
    runId,
    stepId,
    section,
    nodeRunId,
    cursor,
    getApplicationsApiBaseUrl()
  );
}

export function fetchWorkflowTrajectory(
  applicationId: string,
  runId: string,
  cursor?: string,
  options?: WorkflowTrajectoryOptions
) {
  return getConsoleWorkflowTrajectory(
    applicationId,
    runId,
    cursor,
    options,
    getApplicationsApiBaseUrl()
  );
}
export function fetchWorkflowTrajectoryBody(
  applicationId: string,
  runId: string,
  eventId: string
) {
  return getConsoleWorkflowTrajectoryBody(
    applicationId,
    runId,
    eventId,
    getApplicationsApiBaseUrl()
  );
}

export function fetchApplicationLogRecord(
  applicationId: string,
  recordId: string
) {
  return getConsoleApplicationLogRecord(
    applicationId,
    recordId,
    getApplicationsApiBaseUrl()
  );
}
export function fetchApplicationLogRecordConversationMessages(
  applicationId: string,
  recordId: string,
  input: ApplicationLogRecordConversationQuery = {}
) {
  return getConsoleApplicationLogRecordConversationMessages(
    applicationId,
    recordId,
    input,
    getApplicationsApiBaseUrl()
  );
}
export function fetchApplicationLogRecordClientTrajectory(
  applicationId: string,
  recordId: string,
  cursor?: RecordClientTrajectoryCursor,
  options?: ClientTrajectoryOptions
) {
  return getConsoleApplicationLogRecordClientTrajectory(
    applicationId,
    recordId,
    cursor,
    options,
    getApplicationsApiBaseUrl()
  );
}
export function fetchApplicationLogRecordClientTrajectorySection(
  applicationId: string,
  recordId: string,
  stepId: string,
  section: string,
  cursor?: number
) {
  return getConsoleApplicationLogRecordClientTrajectorySection(
    applicationId,
    recordId,
    stepId,
    section,
    cursor,
    getApplicationsApiBaseUrl()
  );
}

export function fetchApplicationLogRecordTraceTree(
  applicationId: string,
  recordId: string
) {
  return getConsoleApplicationLogRecordTraceTree(
    applicationId,
    recordId,
    getApplicationsApiBaseUrl()
  );
}
export function fetchApplicationLogRecordTraceChildren(
  applicationId: string,
  recordId: string,
  traceNodeId: string,
  cursor?: string
) {
  return getConsoleApplicationLogRecordTraceChildren(
    applicationId,
    recordId,
    traceNodeId,
    cursor,
    20,
    getApplicationsApiBaseUrl()
  );
}

export function fetchConversationClientTrajectory(
  applicationId: string,
  logConversationId: string,
  input: ConversationTrajectoryQuery
) {
  return queryConsoleConversationClientTrajectory(
    applicationId,
    logConversationId,
    input,
    getApplicationsApiBaseUrl()
  );
}
export function fetchConversationClientTrajectorySection(
  applicationId: string,
  logConversationId: string,
  stepId: string,
  section: string,
  cursor?: number
) {
  return getConsoleConversationClientTrajectorySection(
    applicationId,
    logConversationId,
    stepId,
    section,
    cursor,
    getApplicationsApiBaseUrl()
  );
}
