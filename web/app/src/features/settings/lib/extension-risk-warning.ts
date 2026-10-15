function extensionRiskWarningKey(code: string) {
  switch (code) {
    case 'checksum_missing':
      return 'auto.extension_warning_checksum_missing';
    case 'signature_missing':
      return 'auto.extension_warning_signature_missing';
    case 'signing_key_unknown':
      return 'auto.extension_warning_signing_key_unknown';
    default:
      return null;
  }
}

export function extensionRiskWarningText(
  code: string,
  message: string,
  t: (key: string) => string
) {
  const key = extensionRiskWarningKey(code);
  return key ? t(key) : message;
}
