import { useCallback, useEffect, useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  Alert,
  Button,
  Empty,
  Flex,
  Grid,
  Popconfirm,
  Space,
  Tag,
  Typography
} from 'antd';
import { useTranslation } from 'react-i18next';
import type { NotificationInstance } from '@1flowbase/api-client';
import { useAuthStore } from '../../../../state/auth-store';
import { selectNavigationQueryScope } from '../../../../state/navigation-query-scope';
import { SettingsSectionSurface } from '../../components/SettingsSectionSurface';
import {
  notificationInstancesOptions,
  notificationTypesOptions,
  notificationQueryKey,
  updateNotificationInstance,
  deleteNotificationInstance
} from '../../../notifications/api/queries';
import {
  DataTable,
  type DataTableColumn
} from '../../../../shared/ui/data-table/DataTable';
import { usePersistedDataTableConfiguration } from '../../../../shared/ui/data-table/data-table-state';
import { NotificationInstanceDrawer } from './NotificationInstanceDrawer';

export function NotificationsPage() {
  const { t } = useTranslation('notifications');
  const scope = useAuthStore(selectNavigationQueryScope);
  const csrfToken = useAuthStore((state) => state.csrfToken) ?? '';
  const client = useQueryClient();
  const instances = useQuery(notificationInstancesOptions(scope));
  const types = useQuery(notificationTypesOptions(scope));
  const [editing, setEditing] = useState<NotificationInstance | 'new'>();
  const refresh = () =>
    client.invalidateQueries({ queryKey: notificationQueryKey(scope) });
  const status = useMutation({
    mutationFn: (instance: NotificationInstance) =>
      updateNotificationInstance(
        instance.id,
        { status: instance.status === 'enabled' ? 'disabled' : 'enabled' },
        csrfToken
      ),
    onSuccess: refresh
  });
  const remove = useMutation({
    mutationFn: (id: string) => deleteNotificationInstance(id, csrfToken),
    onSuccess: refresh
  });
  const availability = useCallback(
    (instance: NotificationInstance) => {
      if (instance.available) return null;
      switch (instance.unavailable_reason) {
        case 'instance_disabled':
          return t('instance_disabled');
        case 'plugin_unavailable':
          return t('plugin_unavailable');
        case 'type_unavailable':
          return t('type_unavailable');
        default:
          return t('type_unavailable');
      }
    },
    [t]
  );
  const mobile = Grid.useBreakpoint().md === false;
  const [page, setPage] = useState(1);
  useEffect(() => {
    setEditing(undefined);
    setPage(1);
  }, [scope]);
  const columns = useMemo<Array<DataTableColumn<NotificationInstance>>>(
    () => [
      {
        key: 'display_name',
        width: 200,
        title: t('display_name'),
        dataIndex: 'display_name',
        mobileRole: 'title',
        render: (_, item) => (
          <Button type="link" onClick={() => setEditing(item)}>
            {item.display_name}
          </Button>
        )
      },
      {
        key: 'type_code',
        width: 160,
        title: t('type'),
        dataIndex: 'type_code',
        render: (_, item) =>
          types.data?.find(
            (type) =>
              type.type_code === item.type_code &&
              type.installation_id === item.installation_id
          )?.display_name ?? item.type_code
      },
      {
        key: 'status',
        width: 180,
        title: t('status'),
        render: (_, item) => (
          <Space orientation="vertical" size="small">
            <Tag>
              {item.status === 'enabled' ? t('enabled') : t('disabled')}
            </Tag>
            {availability(item) && (
              <Typography.Text type="secondary">
                {availability(item)}
              </Typography.Text>
            )}
          </Space>
        )
      },
      {
        key: 'actions',
        width: 240,
        mobileRole: 'actions',
        title: t('actions'),
        render: (_, item) => (
          <Space wrap>
            <Button onClick={() => setEditing(item)}>{t('edit')}</Button>
            <Button
              danger={item.status === 'enabled'}
              loading={status.isPending && status.variables?.id === item.id}
              onClick={() => status.mutate(item)}
            >
              {item.status === 'enabled' ? t('disable') : t('enable')}
            </Button>
            <Popconfirm
              title={t('delete_confirm')}
              onConfirm={() => remove.mutateAsync(item.id)}
            >
              <Button
                danger
                loading={remove.isPending && remove.variables === item.id}
              >
                {t('delete')}
              </Button>
            </Popconfirm>
          </Space>
        )
      }
    ],
    [
      t,
      availability,
      types.data,
      status.mutate,
      status.isPending,
      status.variables,
      remove.mutateAsync,
      remove.isPending,
      remove.variables
    ]
  );
  const configuration = usePersistedDataTableConfiguration({
    columns,
    storageKey: 'notifications.instances.columns'
  });
  return (
    <SettingsSectionSurface
      toolbar={
        <Flex gap="small" wrap justify="space-between" align="center">
          <Typography.Text strong>{t('instances')}</Typography.Text>
          <Space wrap>
            <Button onClick={() => void refresh()}>{t('refresh')}</Button>
            <Button
              type="primary"
              disabled={!types.data?.some((type) => type.available)}
              onClick={() => setEditing('new')}
            >
              {t('create_instance')}
            </Button>
          </Space>
        </Flex>
      }
      status={
        (instances.isError ||
          types.isError ||
          status.isError ||
          remove.isError) && (
          <Alert type="error" showIcon title={t('instances_failed')} />
        )
      }
    >
      <DataTable<NotificationInstance>
        columns={columns}
        configuration={configuration}
        rowKey="id"
        dataSource={(instances.data ?? []).slice(
          mobile ? 0 : (page - 1) * 20,
          page * 20
        )}
        mobileList={
          mobile
            ? {
                resetKey: scope,
                hasMore: page * 20 < (instances.data?.length ?? 0),
                loading: instances.isPending,
                failed: instances.isError,
                onLoadMore: async () => setPage((current) => current + 1)
              }
            : undefined
        }
        loading={instances.isPending}
        page={page}
        pageSize={20}
        total={instances.data?.length ?? 0}
        onPageChange={setPage}
        emptyText={<Empty description={t('no_instances')} />}
      />
      {editing && (
        <NotificationInstanceDrawer
          key={editing === 'new' ? 'new' : editing.id}
          instance={editing === 'new' ? undefined : editing}
          types={types.data ?? []}
          onClose={() => setEditing(undefined)}
          onSaved={() => {
            setEditing(undefined);
            void refresh();
          }}
        />
      )}
    </SettingsSectionSurface>
  );
}
