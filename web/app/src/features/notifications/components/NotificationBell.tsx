import { useState } from 'react';
import BellOutlined from '@ant-design/icons/es/icons/BellOutlined';
import {
  Alert,
  Badge,
  Button,
  Divider,
  Drawer,
  Empty,
  Flex,
  Space,
  Spin,
  Switch,
  Typography
} from 'antd';
import { useTranslation } from 'react-i18next';
import { formatDateTime } from '../../../shared/i18n/format';
import { useNotificationInbox } from '../hooks/use-notification-inbox';
import './notification-bell.css';

export function NotificationBell() {
  const { t } = useTranslation('notifications');
  const [open, setOpen] = useState(false);
  const inbox = useNotificationInbox(open);
  if (!inbox.authenticated) return null;
  const failed =
    inbox.messages.isError ||
    inbox.unread.isError ||
    inbox.read.isError ||
    inbox.readAll.isError;
  return (
    <>
      <Badge count={inbox.unread.data?.unread_count} size="small">
        <Button
          type="text"
          icon={<BellOutlined />}
          aria-label={t('inbox')}
          onClick={() => setOpen(true)}
        />
      </Badge>
      <Drawer
        title={t('inbox')}
        open={open}
        onClose={() => setOpen(false)}
        size={440}
        className="notification-inbox"
      >
        <Space
          orientation="vertical"
          size="middle"
          className="notification-inbox__body"
        >
          <Flex gap="small" wrap align="center">
            <Switch
              checked={inbox.unread_only}
              onChange={inbox.setUnreadOnly}
              aria-label={t('unread_only')}
            />
            <span>{t('unread_only')}</span>
            <Button onClick={() => void inbox.refresh()}>{t('refresh')}</Button>
            <Button
              disabled={!inbox.unread.data?.unread_count}
              loading={inbox.readAll.isPending}
              onClick={() => inbox.readAll.mutate()}
            >
              {t('read_all')}
            </Button>
          </Flex>
          {!inbox.connected && <Alert type="info" title={t('reconnecting')} />}
          {failed && <Alert type="error" showIcon title={t('inbox_failed')} />}
          {inbox.messages.isPending ? (
            <Spin />
          ) : inbox.messages.data?.items.length === 0 ? (
            <Empty description={t('empty')} />
          ) : (
            inbox.messages.data?.items.map((item) => (
              <article key={item.id}>
                <Flex gap="small" justify="space-between" align="start">
                  <Typography.Text strong>{item.title}</Typography.Text>
                  <Badge
                    status={item.read_at ? 'default' : 'processing'}
                    text={item.read_at ? t('read') : t('unread')}
                  />
                </Flex>
                <Typography.Paragraph className="notification-inbox__content">
                  {item.content}
                </Typography.Paragraph>
                <Flex justify="space-between" align="center" gap="small">
                  <Typography.Text type="secondary">
                    {formatDateTime(item.created_at)}
                  </Typography.Text>
                  {!item.read_at && (
                    <Button
                      size="small"
                      loading={
                        inbox.read.isPending && inbox.read.variables === item.id
                      }
                      onClick={() => inbox.read.mutate(item.id)}
                    >
                      {t('mark_read')}
                    </Button>
                  )}
                </Flex>
                <Divider />
              </article>
            ))
          )}
          <Flex gap="small" justify="end">
            <Button disabled={!inbox.hasPrevious} onClick={inbox.previous}>
              {t('previous')}
            </Button>
            <Button
              disabled={!inbox.messages.data?.next_cursor}
              onClick={inbox.next}
            >
              {t('next')}
            </Button>
          </Flex>
        </Space>
      </Drawer>
    </>
  );
}
