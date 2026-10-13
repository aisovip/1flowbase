import { useState } from 'react';
import BellOutlined from '@ant-design/icons/es/icons/BellOutlined';
import {
  Alert,
  Badge,
  Button,
  Divider,
  Empty,
  Flex,
  Space,
  Spin,
  Tabs,
  Typography
} from 'antd';
import { useTranslation } from 'react-i18next';
import type { NotificationReadStatus } from '@1flowbase/api-client';
import { formatDateTime } from '../../../shared/i18n/format';
import { ResizableDrawer } from '../../../shared/ui/resizable-drawer/ResizableDrawer';
import { useNotificationInbox } from '../hooks/use-notification-inbox';
import './notification-bell.css';

export function NotificationBell() {
  const { t } = useTranslation('notifications');
  const [open, setOpen] = useState(false);
  const inbox = useNotificationInbox(open);
  if (!inbox.authenticated) return null;
  const failed =
    inbox.channels.isError ||
    inbox.messages.isError ||
    inbox.unread.isError ||
    inbox.write.isError;
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
      <ResizableDrawer
        title={t('inbox')}
        open={open}
        onClose={() => setOpen(false)}
        defaultWidth={900}
        minWidth={480}
        maxWidth={1200}
        resizeLabel={t('resize_drawer')}
        rootClassName="notification-inbox"
        bodyClassName="notification-inbox__body"
      >
        {!inbox.connected && <Alert type="info" title={t('reconnecting')} />}
        {failed && <Alert type="error" showIcon title={t('inbox_failed')} />}
        <div
          className={`notification-inbox__workspace${inbox.selected ? ' notification-inbox__workspace--selected' : ''}`}
        >
          <aside
            className="notification-inbox__channels"
            aria-label={t('channels')}
          >
            <Tabs
              activeKey={inbox.read_status}
              onChange={(value) =>
                inbox.setReadStatus(value as NotificationReadStatus)
              }
              items={[
                { key: 'all', label: t('all_messages') },
                { key: 'unread', label: t('unread') },
                { key: 'read', label: t('read') }
              ]}
            />
            <div className="notification-inbox__channel-list">
              {inbox.channels.isPending ? (
                <Spin />
              ) : inbox.channels.data?.items.length === 0 ? (
                <Empty description={t('no_channels')} />
              ) : (
                inbox.channels.data?.items.map((channel) => (
                  <button
                    type="button"
                    key={channel.instance_id}
                    className="notification-inbox__channel"
                    aria-pressed={
                      inbox.selected?.instance_id === channel.instance_id
                    }
                    onClick={() => inbox.select(channel)}
                  >
                    <Flex justify="space-between" gap="small">
                      <span className="notification-inbox__channel-name">
                        {channel.display_name}
                      </span>
                      <Badge count={channel.unread_count} size="small" />
                    </Flex>
                    {channel.deleted && (
                      <span className="notification-inbox__muted">
                        {t('deleted_channel')}
                      </span>
                    )}
                    <span className="notification-inbox__summary">
                      {channel.latest_title}
                    </span>
                    <span className="notification-inbox__summary notification-inbox__muted">
                      {channel.latest_content}
                    </span>
                    <time className="notification-inbox__muted">
                      {formatDateTime(channel.latest_created_at)}
                    </time>
                  </button>
                ))
              )}
            </div>
            <Flex
              gap="small"
              justify="end"
              className="notification-inbox__pagination"
            >
              <Button
                disabled={
                  !inbox.channelsHasPrevious || inbox.channels.isFetching
                }
                onClick={inbox.previousChannels}
              >
                {t('previous')}
              </Button>
              <Button
                disabled={
                  !inbox.channels.data?.next_cursor || inbox.channels.isFetching
                }
                onClick={inbox.nextChannels}
              >
                {t('next')}
              </Button>
            </Flex>
          </aside>
          <section
            className="notification-inbox__messages"
            aria-label={t('channel_messages')}
          >
            <Flex
              gap="small"
              justify="space-between"
              align="center"
              wrap
              className="notification-inbox__toolbar"
            >
              <Button
                className="notification-inbox__back"
                onClick={() => inbox.select(undefined)}
              >
                {t('back_channels')}
              </Button>
              {inbox.selected && (
                <Typography.Text strong>
                  {inbox.selected.display_name}
                </Typography.Text>
              )}
              <Space wrap>
                <Button onClick={() => void inbox.refresh()}>
                  {t('refresh')}
                </Button>
                <Button
                  disabled={
                    !inbox.selected ||
                    !inbox.messages.data?.unread_count ||
                    inbox.write.isPending
                  }
                  loading={
                    inbox.write.isPending &&
                    inbox.write.variables?.operation === 'channel'
                  }
                  onClick={inbox.readAll}
                >
                  {t('read_all')}
                </Button>
              </Space>
            </Flex>
            <div className="notification-inbox__message-list">
              {!inbox.selected ? (
                <Empty description={t('select_channel')} />
              ) : inbox.messages.isPending ? (
                <Spin />
              ) : inbox.messages.data?.items.length === 0 ? (
                <Empty description={t('empty')} />
              ) : (
                inbox.messages.data?.items.map((item) => (
                  <article key={item.id}>
                    <Flex justify="space-between" align="start" gap="small">
                      <Button
                        type="link"
                        className="notification-inbox__message-title"
                        disabled={inbox.write.isPending}
                        onClick={() => {
                          if (!item.read_at) inbox.markRead(item.id);
                        }}
                      >
                        {item.title}
                      </Button>
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
                      <Button
                        size="small"
                        disabled={inbox.write.isPending}
                        loading={
                          inbox.write.isPending &&
                          inbox.write.variables?.id === item.id
                        }
                        onClick={() =>
                          item.read_at
                            ? inbox.markUnread(item.id)
                            : inbox.markRead(item.id)
                        }
                      >
                        {item.read_at ? t('mark_unread') : t('mark_read')}
                      </Button>
                    </Flex>
                    <Divider />
                  </article>
                ))
              )}
            </div>
            {inbox.selected && (
              <Flex
                gap="small"
                justify="end"
                className="notification-inbox__pagination"
              >
                <Button
                  disabled={
                    !inbox.messagesHasPrevious || inbox.messages.isFetching
                  }
                  onClick={inbox.previousMessages}
                >
                  {t('previous')}
                </Button>
                <Button
                  disabled={
                    !inbox.messages.data?.next_cursor ||
                    inbox.messages.isFetching
                  }
                  onClick={inbox.nextMessages}
                >
                  {t('next')}
                </Button>
              </Flex>
            )}
          </section>
        </div>
      </ResizableDrawer>
    </>
  );
}
