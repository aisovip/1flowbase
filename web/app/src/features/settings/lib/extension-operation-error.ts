import { ApiClientError } from '@1flowbase/api-client';

export function extensionOperationErrorText(
  error: unknown,
  t: (key: string) => string
) {
  if (!(error instanceof ApiClientError))
    return t('auto.extension_operation_failed');
  switch (error.code) {
    case 'extension_artifact_not_published':
      return t('auto.extension_artifact_not_published');
    case 'extension_artifact_network_unavailable':
    case 'extension_artifact_upstream_rejected':
    case 'extension_artifact_download_unavailable':
      return t('auto.extension_artifact_download_failed');
    case 'extension_artifact_checksum_mismatch':
      return t('auto.extension_artifact_checksum_failed');
    case 'extension_artifact_signature_invalid':
      return t('auto.extension_artifact_signature_failed');
    case 'permission_denied':
    case 'console_operation_permission_denied':
      return t('auto.extension_operation_permission_denied');
    case 'contribution_permission_not_delegable':
      return t('auto.extension_capability_not_supported');
    case 'managed_activation_not_configured':
      return t('auto.extension_activation_unavailable');
    case 'managed_contribution_authorization_required':
      return t('auto.extension_contribution_authorization_required');
    case 'managed_archive_identity_conflict':
      return t('auto.extension_archive_identity_conflict');
    default:
      return error.message;
  }
}
