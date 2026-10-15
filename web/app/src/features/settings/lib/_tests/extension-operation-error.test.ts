import { expect, test } from 'vitest';
import { ApiClientError } from '@1flowbase/api-client';
import { extensionOperationErrorText } from '../extension-operation-error';

test.each([
  [
    'managed_contribution_authorization_required',
    'auto.extension_contribution_authorization_required'
  ],
  [
    'managed_archive_identity_conflict',
    'auto.extension_archive_identity_conflict'
  ]
])('explains the actionable failure %s', (code, key) => {
  expect(
    extensionOperationErrorText(
      new ApiClientError({ status: 409, code, message: 'conflict' }),
      (value) => value
    )
  ).toBe(key);
});

test('preserves the server reason for errors without a localized explanation', () => {
  expect(
    extensionOperationErrorText(
      new ApiClientError({
        status: 400,
        code: 'plugin_runtime_target_mismatch',
        message: 'linux-arm64 is incompatible with linux-amd64'
      }),
      (value) => value
    )
  ).toBe('linux-arm64 is incompatible with linux-amd64');
});
