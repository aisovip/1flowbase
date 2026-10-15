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
