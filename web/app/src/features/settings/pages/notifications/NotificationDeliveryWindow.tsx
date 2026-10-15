import { Space, Typography } from 'antd';
import { useTranslation } from 'react-i18next';
import type { NotificationDeliveryWindowView } from '@1flowbase/api-client';
import { formatDateTime } from '../../../../shared/i18n/format';

export function NotificationDeliveryWindow({
  delivery_window,
  compact = false
}: {
  delivery_window: NotificationDeliveryWindowView;
  compact?: boolean;
}) {
  const { t } = useTranslation('notifications');
  const statusLabel =
    delivery_window.status === 'active'
      ? t('delivery_window_active')
      : delivery_window.status === 'expired'
        ? t('delivery_window_expired')
        : delivery_window.status === 'waiting_for_message'
          ? t('delivery_window_waiting')
          : t('delivery_window_unknown');
  return (
    <Space orientation="vertical" size="small">
      <Typography.Text>
        {t('delivery_window_status')}: {statusLabel}
      </Typography.Text>
      {!compact && delivery_window.last_received_at_ms !== undefined && (
        <Typography.Text type="secondary">
          {t('delivery_window_last_received')}:{' '}
          {formatDateTime(new Date(delivery_window.last_received_at_ms))}
        </Typography.Text>
      )}
      {!compact && delivery_window.expires_at_ms !== undefined && (
        <Typography.Text type="secondary">
          {t('delivery_window_expires')}:{' '}
          {formatDateTime(new Date(delivery_window.expires_at_ms))}
        </Typography.Text>
      )}
    </Space>
  );
}
