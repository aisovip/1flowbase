import { useState } from 'react';
import { Image, Typography } from 'antd';
import type { ConsoleFileRecord } from '@1flowbase/api-client';
import { useNativeBlockSurface } from '../../lib/native-modules/native-block-surface-context';

export function FileRecordPreview({ record }: { record: ConsoleFileRecord }) {
  const surface = useNativeBlockSurface();
  const [failedUrl, setFailedUrl] = useState<string>();
  const previewUrl = record.preview_url;

  if (!previewUrl || failedUrl === previewUrl) {
    return (
      <Typography.Link
        href={record.url}
        target="_blank"
        rel="noreferrer"
        ellipsis
        title={record.url}
        style={{ display: 'block', maxWidth: '100%' }}
      >
        {record.url}
      </Typography.Link>
    );
  }

  return (
    <Image
      src={previewUrl}
      alt={record.filename}
      width={88}
      height={56}
      loading="lazy"
      style={{ objectFit: 'cover', borderRadius: 8 }}
      onError={() => setFailedUrl(previewUrl)}
      preview={{ getContainer: surface?.overlayHost.getPopupContainer }}
    />
  );
}
