import { useCallback, useEffect, useMemo, useState } from 'react';
import DownOutlined from '@ant-design/icons/es/icons/DownOutlined';
import PlusOutlined from '@ant-design/icons/es/icons/PlusOutlined';
import ReloadOutlined from '@ant-design/icons/es/icons/ReloadOutlined';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  Alert,
  Button,
  Dropdown,
  Empty,
  Flex,
  Grid,
  Popconfirm,
  Space,
  Switch,
  Tabs,
  Typography
} from 'antd';
import { useTranslation } from 'react-i18next';
import type {
  NotificationInstance,
  NotificationInstanceListInput,
  NotificationType
} from '@1flowbase/api-client';
import { useAuthStore } from '../../../../state/auth-store';
import { selectNavigationQueryScope } from '../../../../state/navigation-query-scope';
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
import { NotificationInstanceFilters } from './NotificationInstanceFilters';
import { NotificationLogs } from './NotificationLogs';
import './notifications-page.css';

export function NotificationsPage() {
  const { t } = useTranslation('notifications');
  const scope = useAuthStore(selectNavigationQueryScope);
  return (
    <section className="notifications-page">
      <Tabs
        key={scope}
        items={[
          {
            key: 'channels',
            label: t('channels'),
            children: <NotificationChannelsPage />
          },
          { key: 'logs', label: t('logs'), children: <NotificationLogs /> }
        ]}
      />
    </section>
  );
}

function NotificationChannelsPage() {
  const { t } = useTranslation('notifications');
  const scope = useAuthStore(selectNavigationQueryScope);
  const csrfToken = useAuthStore((state) => state.csrfToken) ?? '';
  const client = useQueryClient();
  const [filters, setFilters] = useState<NotificationInstanceListInput>({});
  const instances = useQuery(notificationInstancesOptions(scope, filters));
  const types = useQuery(notificationTypesOptions(scope));
  const [editing, setEditing] = useState<{
    instance?: NotificationInstance;
    type?: NotificationType;
  }>();
  const editInstance = useCallback(
    (instance: NotificationInstance) => {
      setEditing({
        instance,
        type: types.data?.find(
          (type) =>
            type.type_code === instance.type_code &&
            type.installation_id === instance.installation_id
        )
      });
    },
    [types.data]
  );
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
          return null;
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
    setFilters({});
    setPage(1);
  }, [scope]);
  const {
    mutate: changeStatus,
    isPending: statusPending,
    variables: statusVariables
  } = status;
  const {
    mutateAsync: deleteInstance,
    isPending: deletePending,
    variables: deleteVariables
  } = remove;
  const columns = useMemo<Array<DataTableColumn<NotificationInstance>>>(
    () => [
      {
        key: 'display_name',
        width: 200,
        title: t('display_name'),
        dataIndex: 'display_name',
        mobileRole: 'title',
        render: (_, item) => (
          <Button
            type="link"
            className="notifications-page__name"
            onClick={() => editInstance(item)}
          >
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
            <Switch
              aria-label={`${item.display_name}: ${t('status')}`}
              checked={item.status === 'enabled'}
              disabled={statusPending}
              loading={statusPending && statusVariables?.id === item.id}
              onChange={() => changeStatus(item)}
            />
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
            <Button
              type="link"
              className="notifications-page__action"
              onClick={() => editInstance(item)}
            >
              {t('edit')}
            </Button>
            <Popconfirm
              title={t('delete_confirm')}
              onConfirm={() => deleteInstance(item.id)}
            >
              <Button
                danger
                type="link"
                className="notifications-page__action"
                loading={deletePending && deleteVariables === item.id}
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
      editInstance,
      availability,
      types.data,
      changeStatus,
      statusPending,
      statusVariables,
      deleteInstance,
      deletePending,
      deleteVariables
    ]
  );
  const configuration = usePersistedDataTableConfiguration({
    columns,
    storageKey: 'notifications.instances.columns'
  });
  return (
    <section className="notifications-page">
      <Typography.Title level={4} className="notifications-page__title">
        {t('management')}
      </Typography.Title>
      {(instances.isError ||
        types.isError ||
        status.isError ||
        remove.isError) && (
        <Alert type="error" showIcon title={t('instances_failed')} />
      )}
      <DataTable<NotificationInstance>
        className="notifications-page__table"
        toolbar={
          <Flex gap="small" wrap justify="space-between" align="center">
            <NotificationInstanceFilters
              types={types.data ?? []}
              value={filters}
              onChange={(value) => {
                setFilters(value);
                setPage(1);
              }}
            />
            <Space wrap>
              <Button
                icon={<ReloadOutlined />}
                loading={instances.isFetching && !instances.isPending}
                onClick={() => void refresh()}
              >
                {t('refresh')}
              </Button>
              <Dropdown
                trigger={['click']}
                disabled={!types.data?.some((type) => type.available)}
                menu={{
                  items: (types.data ?? [])
                    .filter((type) => type.available)
                    .map((type) => ({
                      key: JSON.stringify([
                        type.type_code,
                        type.installation_id
                      ]),
                      label: type.display_name,
                      onClick: () => setEditing({ type })
                    }))
                }}
              >
                <Button
                  type="primary"
                  icon={<PlusOutlined />}
                  disabled={!types.data?.some((type) => type.available)}
                >
                  {t('create')} <DownOutlined />
                </Button>
              </Dropdown>
            </Space>
          </Flex>
        }
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
                resetKey: JSON.stringify([scope, filters]),
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
        emptyText={
          mobile ? (
            t('no_instances')
          ) : (
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description={t('no_instances')}
            />
          )
        }
      />
      {editing && (
        <NotificationInstanceDrawer
          key={
            editing.instance?.id ??
            JSON.stringify([
              editing.type?.type_code,
              editing.type?.installation_id
            ])
          }
          instance={editing.instance}
          type={editing.type}
          onClose={() => setEditing(undefined)}
          onSaved={() => {
            setEditing(undefined);
            void refresh();
          }}
        />
      )}
    </section>
  );
}
