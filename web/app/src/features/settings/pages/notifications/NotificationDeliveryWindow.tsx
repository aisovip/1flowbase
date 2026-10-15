import { Tag, Typography } from 'antd';
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
  if (compact) return <Typography.Text>{statusLabel}</Typography.Text>;
  return (
    <dl className="notification-subscription">
      <div className="notification-subscription__row">
        <dt>{t('delivery_window_status')}</dt>
        <dd>
          <Tag
            className="notification-status"
            color={
              delivery_window.status === 'active'
                ? 'success'
                : delivery_window.status === 'expired'
                  ? 'error'
                  : delivery_window.status === 'waiting_for_message'
                    ? 'warning'
                    : 'default'
            }
          >
            {statusLabel}
          </Tag>
        </dd>
      </div>
      {delivery_window.last_received_at_ms !== undefined && (
        <div className="notification-subscription__row">
          <dt>{t('delivery_window_last_received')}</dt>
          <dd>
            {formatDateTime(new Date(delivery_window.last_received_at_ms))}
          </dd>
        </div>
      )}
      {delivery_window.expires_at_ms !== undefined && (
        <div className="notification-subscription__row">
          <dt>{t('delivery_window_expires')}</dt>
          <dd>{formatDateTime(new Date(delivery_window.expires_at_ms))}</dd>
        </div>
      )}
    </dl>
  );
}
