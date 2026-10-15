import {
  exportConsoleApplicationLogsOtlp,
  importConsoleApplicationLogsOtlp,
  type ApplicationLogExportSelector
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
