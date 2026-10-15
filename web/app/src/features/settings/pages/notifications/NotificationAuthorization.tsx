import { useCallback, useEffect, useRef, useState } from 'react';
import { Alert, Button, QRCode, Space, Typography } from 'antd';
import { useTranslation } from 'react-i18next';
import type {
  NotificationAuthorizationDescriptor,
  NotificationAuthorizationOperation,
  NotificationAuthorizationView
} from '@1flowbase/api-client';
import { authorizeNotificationInstance } from '../../../notifications/api/queries';
import { useAuthStore } from '../../../../state/auth-store';

export function NotificationAuthorization({
  instance_id,
  descriptor,
  authorization,
  beginOnMount = false,
  onChanged
}: {
  instance_id: string;
  descriptor: NotificationAuthorizationDescriptor;
  authorization: NotificationAuthorizationView;
  beginOnMount?: boolean;
  onChanged: () => void;
}) {
  const { t } = useTranslation('notifications');
  const csrfToken = useAuthStore((state) => state.csrfToken) ?? '';
  const [view, setView] = useState(authorization);
  const [busy, setBusy] =
    useState<NotificationAuthorizationOperation['type']>();
  const [failed, setFailed] = useState(false);
  const request = useRef<AbortController | null>(null);
  const changed = useRef(onChanged);
  changed.current = onChanged;
  const command = useCallback(
    async (operation: NotificationAuthorizationOperation) => {
      request.current?.abort();
      const controller = new AbortController();
      request.current = controller;
      setBusy(operation.type);
      setFailed(false);
      try {
        const result = await authorizeNotificationInstance(
          instance_id,
          operation,
          csrfToken,
          controller.signal
        );
        if (controller.signal.aborted) return;
        setView(result);
        changed.current();
      } catch {
        if (!controller.signal.aborted) setFailed(true);
      } finally {
        if (!controller.signal.aborted) setBusy(undefined);
      }
    },
    [instance_id, csrfToken]
  );

  useEffect(() => {
    request.current?.abort();
    setView(authorization);
    setBusy(undefined);
    setFailed(false);
  }, [authorization]);
  useEffect(() => () => request.current?.abort(), []);
  const firstAction = descriptor.actions.find(
    (action) => action.kind === 'qr_code'
  );
  const autoStarted = useRef(false);
  useEffect(() => {
    if (beginOnMount && firstAction && !autoStarted.current) {
      const timer = window.setTimeout(() => {
        autoStarted.current = true;
        void command({ type: 'begin', action: firstAction.code });
      }, 0);
      return () => window.clearTimeout(timer);
    }
  }, [beginOnMount, firstAction, command]);
  useEffect(() => {
    if (view.status !== 'pending' || !view.user_action || failed || busy)
      return;
    const timer = window.setTimeout(() => {
      void command({ type: 'poll' });
    }, view.user_action.poll_interval_seconds * 1000);
    return () => window.clearTimeout(timer);
  }, [view, failed, busy, command]);

  const statusLabel =
    view.status === 'authorized'
      ? t('authorization_authorized')
      : view.status === 'pending'
        ? t('authorization_pending')
        : view.status === 'invalid'
          ? t('authorization_invalid')
          : t('authorization_unauthorized');
  return (
    <Space orientation="vertical" size="middle" style={{ width: '100%' }}>
      <Typography.Text strong>
        {t('authorization_status')}: {statusLabel}
      </Typography.Text>
      {failed && (
        <Alert type="error" showIcon title={t('authorization_failed')} />
      )}
      {view.status === 'pending' && view.user_action?.kind === 'qr_code' && (
        <>
          <QRCode value={view.user_action.qr_code_url} />
          <Typography.Text>{t('authorization_waiting')}</Typography.Text>
          <Typography.Text type="secondary">
            {t('authorization_not_delivery')}
          </Typography.Text>
          <Space wrap>
            <Button
              loading={busy === 'cancel'}
              onClick={() => void command({ type: 'cancel' })}
            >
              {t('authorization_cancel')}
            </Button>
            {failed && (
              <Button onClick={() => void command({ type: 'poll' })}>
                {t('refresh')}
              </Button>
            )}
          </Space>
        </>
      )}
      {view.status === 'authorized' && (
        <Alert
          type="info"
          showIcon
          title={t('authorization_ready')}
          description={t('authorization_session_hint')}
        />
      )}
      {view.status !== 'pending' && (
        <Space wrap>
          {descriptor.actions
            .filter((action) => action.kind === 'qr_code')
            .map((action) => (
              <Button
                key={action.code}
                disabled={!!busy}
                loading={busy === 'begin'}
                onClick={() =>
                  void command({ type: 'begin', action: action.code })
                }
              >
                {view.status === 'invalid' || view.status === 'authorized'
                  ? t('authorization_reauthorize', { action: action.label })
                  : action.label}
              </Button>
            ))}
        </Space>
      )}
    </Space>
  );
}
