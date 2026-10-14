import {
  exportConsoleApplicationLogsOtlp,
  importConsoleApplicationLogsOtlp,
  exportConsoleApplicationLogsLangfuse,
  importConsoleApplicationLogsLangfuse,
  type ApplicationLogLangfuseCredentials
} from '@1flowbase/api-client';
import { getApplicationsApiBaseUrl } from './applications';
export function exportApplicationLogsOtlp(
  applicationId: string,
  record_ids: string[],
  csrfToken: string
) {
  return exportConsoleApplicationLogsOtlp(
    applicationId,
    { record_ids },
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
  record_ids: string[],
  credentials: ApplicationLogLangfuseCredentials,
  csrfToken: string
) {
  return exportConsoleApplicationLogsLangfuse(
    applicationId,
    { record_ids, ...credentials },
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
