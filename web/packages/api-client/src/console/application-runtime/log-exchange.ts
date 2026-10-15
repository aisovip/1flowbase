import { apiFetch, apiFetchBlob } from '../../transport';

export type ApplicationLogExportSelector =
  | { record_ids: string[]; log_conversation_ids?: never }
  | { log_conversation_ids: string[]; record_ids?: never };

export interface ApplicationLogImportReceipt {
  accepted_events: number;
  duplicate_events: number;
  record_ids: string[];
}
export interface ApplicationLogPartialImportReceipt {
  accepted_events: number;
  duplicate_events: number;
  committed_records: number;
}
export interface ApplicationLogLangfuseCredentials {
  base_url: string;
  public_key: string;
  secret_key: string;
}
export interface ApplicationLogLangfuseExportReceipt {
  exported_records: number;
  exported_spans: number;
  trace_ids: string[];
}
export function exportConsoleApplicationLogsOtlp(
  applicationId: string,
  input: ApplicationLogExportSelector,
  csrfToken: string,
  baseUrl?: string
) {
  return apiFetchBlob({
    path: `/api/console/applications/${applicationId}/logs/otlp/export`,
    method: 'POST',
    body: input,
    csrfToken,
    baseUrl
  });
}
export function importConsoleApplicationLogsOtlp(
  applicationId: string,
  input: { otlp: object },
  csrfToken: string,
  baseUrl?: string
) {
  return apiFetch<ApplicationLogImportReceipt>({
    path: `/api/console/applications/${applicationId}/logs/otlp/import`,
    method: 'POST',
    body: input,
    csrfToken,
    baseUrl
  });
}
export function exportConsoleApplicationLogsLangfuse(
  applicationId: string,
  input: ApplicationLogLangfuseCredentials & ApplicationLogExportSelector,
  csrfToken: string,
  baseUrl?: string
) {
  return apiFetch<ApplicationLogLangfuseExportReceipt>({
    path: `/api/console/applications/${applicationId}/logs/langfuse/export`,
    method: 'POST',
    body: input,
    csrfToken,
    baseUrl
  });
}
export function importConsoleApplicationLogsLangfuse(
  applicationId: string,
  input: ApplicationLogLangfuseCredentials & { trace_ids: string[] },
  csrfToken: string,
  baseUrl?: string
) {
  return apiFetch<ApplicationLogImportReceipt>({
    path: `/api/console/applications/${applicationId}/logs/langfuse/import`,
    method: 'POST',
    body: input,
    csrfToken,
    baseUrl
  });
}
