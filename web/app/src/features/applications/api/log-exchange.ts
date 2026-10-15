import {
  exportConsoleApplicationLogsOtlp,
  importConsoleApplicationLogsOtlp,
  exportConsoleApplicationLogsLangfuse,
  importConsoleApplicationLogsLangfuse,
  type ApplicationLogExportSelector,
  type ApplicationLogLangfuseCredentials
} from '@1flowbase/api-client';
import { getApplicationsApiBaseUrl } from './applications';
export function exportApplicationLogsOtlp(
  applicationId: string,
  selection: string[] | ApplicationLogExportSelector,
  csrfToken: string
) {
  return exportConsoleApplicationLogsOtlp(
    applicationId,
    Array.isArray(selection) ? { record_ids: selection } : selection,
    csrfToken,
    getApplicationsApiBaseUrl()
  );
}
export function importApplicationLogsOtlp(
  applicationId: string,
  otlp: object,
  csrfToken: string
) {
  return importConsoleApplicationLogsOtlp(
    applicationId,
    { otlp },
    csrfToken,
    getApplicationsApiBaseUrl()
  );
}
export function exportApplicationLogsLangfuse(
  applicationId: string,
  selection: string[] | ApplicationLogExportSelector,
  credentials: ApplicationLogLangfuseCredentials,
  csrfToken: string
) {
  return exportConsoleApplicationLogsLangfuse(
    applicationId,
    {
      ...(Array.isArray(selection) ? { record_ids: selection } : selection),
      ...credentials
    },
    csrfToken,
    getApplicationsApiBaseUrl()
  );
}
export function importApplicationLogsLangfuse(
  applicationId: string,
  trace_ids: string[],
  credentials: ApplicationLogLangfuseCredentials,
  csrfToken: string
) {
  return importConsoleApplicationLogsLangfuse(
    applicationId,
    { trace_ids, ...credentials },
    csrfToken,
    getApplicationsApiBaseUrl()
  );
}
