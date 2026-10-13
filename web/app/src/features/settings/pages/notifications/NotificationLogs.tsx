import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import {
  Alert,
  Button,
  DatePicker,
  Descriptions,
  Empty,
  Flex,
  Select,
  Spin,
  Typography
} from 'antd';
import { useTranslation } from 'react-i18next';
import type {
  NotificationLogListInput,
  NotificationSendLog
} from '@1flowbase/api-client';
import { useAuthStore } from '../../../../state/auth-store';
import { selectNavigationQueryScope } from '../../../../state/navigation-query-scope';
import { formatDateTime } from '../../../../shared/i18n/format';
import { ResizableDrawer } from '../../../../shared/ui/resizable-drawer/ResizableDrawer';
import {
  DataTable,
  type DataTableColumn
} from '../../../../shared/ui/data-table/DataTable';
import {
  notificationLogsOptions,
  notificationLogOptions,
  notificationInstancesOptions,
  notificationTypesOptions
} from '../../../notifications/api/queries';

export function NotificationLogs() {
  const { t } = useTranslation('notifications');
  const scope = useAuthStore(selectNavigationQueryScope);
  const [filters, setFilters] = useState<NotificationLogListInput>({});
  const [page, setPage] = useState(1);
  const [selected, setSelected] = useState<string>();
  const logs = useQuery(
    notificationLogsOptions(scope, { ...filters, page, page_size: 20 })
  );
  const instances = useQuery(notificationInstancesOptions(scope));
  const types = useQuery(notificationTypesOptions(scope));
  const statusLabel = (status: NotificationSendLog['status']) =>
    status === 'success'
      ? t('send_success')
      : status === 'failure'
        ? t('send_failure')
        : t('result_unconfirmed');
  const failureLabel = (code: string | null) => {
    switch (code) {
      case null:
        return '—';
      case 'instance_missing':
        return t('failure_instance_missing');
      case 'instance_disabled':
        return t('instance_disabled');
      case 'type_unavailable':
        return t('type_unavailable');
      case 'plugin_unavailable':
        return t('plugin_unavailable');
      case 'input_invalid':
        return t('failure_input_invalid');
      case 'config_invalid':
        return t('failure_config_invalid');
      case 'provider_result_unknown':
        return t('failure_provider_result_unknown');
      case 'provider_output_invalid':
        return t('failure_provider_output_invalid');
      default:
        return t('failure_reason_unavailable');
    }
  };
  const change = (value: NotificationLogListInput) => {
    setFilters(value);
    setPage(1);
  };
  const columns: Array<DataTableColumn<NotificationSendLog>> = [
    {
      key: 'created_at',
      title: t('sent_at'),
      width: 190,
      render: (_, item) => formatDateTime(item.created_at)
    },
    {
      key: 'source',
      title: t('source'),
      width: 120,
      render: (_, item) =>
        item.source === 'workflow' ? t('source_workflow') : t('source_api')
    },
    {
      key: 'display_name',
      title: t('display_name'),
      width: 180,
      render: (_, item) => item.display_name ?? t('deleted_channel')
    },
    { key: 'type_code', title: t('type'), width: 140, dataIndex: 'type_code' },
    {
      key: 'status',
      title: t('send_status'),
      width: 140,
      render: (_, item) => statusLabel(item.status)
    },
    {
      key: 'failure_code',
      title: t('failure_reason'),
      width: 220,
      render: (_, item) => failureLabel(item.failure_code)
    },
    {
      key: 'actions',
      title: t('actions'),
      width: 100,
      mobileRole: 'actions',
      render: (_, item) => (
        <Button type="link" onClick={() => setSelected(item.id)}>
          {t('detail')}
        </Button>
      )
    }
  ];
  return (
    <section className="notifications-page">
      {logs.isError && <Alert type="error" showIcon title={t('logs_failed')} />}
      <DataTable<NotificationSendLog>
        className="notifications-page__table"
        columns={columns}
        rowKey="id"
        dataSource={logs.data?.items ?? []}
        loading={logs.isPending}
        page={logs.data?.page ?? page}
        pageSize={logs.data?.page_size ?? 20}
        total={logs.data?.total ?? 0}
        onPageChange={setPage}
        emptyText={<Empty description={t('no_logs')} />}
        toolbar={
          <Flex gap="small" wrap>
            <Select
              className="notification-logs__filter"
              allowClear
              aria-label={t('source')}
              placeholder={t('source')}
              value={filters.source}
              options={[
                { value: 'api', label: t('source_api') },
                { value: 'workflow', label: t('source_workflow') }
              ]}
              onChange={(source) => change({ ...filters, source })}
            />
            <Select
              className="notification-logs__filter"
              allowClear
              aria-label={t('send_status')}
              placeholder={t('send_status')}
              value={filters.status}
              options={[
                { value: 'success', label: t('send_success') },
                { value: 'failure', label: t('send_failure') },
                { value: 'pending', label: t('status_pending') },
                { value: 'unknown', label: t('status_unknown') }
              ]}
              onChange={(status) => change({ ...filters, status })}
            />
            <Select
              className="notification-logs__filter"
              allowClear
              aria-label={t('display_name')}
              placeholder={t('display_name')}
              value={filters.instance_id}
              options={instances.data?.map((item) => ({
                value: item.id,
                label: item.display_name
              }))}
              onChange={(instance_id) => change({ ...filters, instance_id })}
            />
            <Select
              className="notification-logs__filter"
              allowClear
              aria-label={t('type')}
              placeholder={t('type')}
              value={filters.type_code}
              options={Array.from(
                new Map(
                  types.data?.map((item) => [
                    item.type_code,
                    { value: item.type_code, label: item.display_name }
                  ])
                ).values()
              )}
              onChange={(type_code) => change({ ...filters, type_code })}
            />
            <DatePicker.RangePicker
              aria-label={t('sent_at')}
              showTime
              onChange={(range) =>
                change({
                  ...filters,
                  created_from: range?.[0]?.toISOString(),
                  created_to: range?.[1]?.toISOString()
                })
              }
            />
            <Button
              onClick={() => void logs.refetch()}
              loading={logs.isFetching}
            >
              {t('refresh')}
            </Button>
          </Flex>
        }
      />
      {selected && (
        <NotificationLogDetail
          id={selected}
          onClose={() => setSelected(undefined)}
          statusLabel={statusLabel}
          failureLabel={failureLabel}
        />
      )}
    </section>
  );
}
function NotificationLogDetail({
  id,
  onClose,
  statusLabel,
  failureLabel
}: {
  id: string;
  onClose: () => void;
  statusLabel: (status: NotificationSendLog['status']) => string;
  failureLabel: (code: string | null) => string;
}) {
  const { t } = useTranslation('notifications');
  const scope = useAuthStore(selectNavigationQueryScope);
  const detail = useQuery(notificationLogOptions(scope, id));
  const item = detail.data;
  return (
    <ResizableDrawer
      open
      title={t('log_detail')}
      onClose={onClose}
      resizeLabel={t('resize_drawer')}
      rootClassName="notification-log-detail"
    >
      {detail.isError && <Alert type="error" title={t('logs_failed')} />}
      {detail.isPending && <Spin />}
      {item && (
        <>
          <Descriptions
            column={1}
            items={[
              {
                key: 'created_at',
                label: t('sent_at'),
                children: formatDateTime(item.created_at)
              },
              {
                key: 'source',
                label: t('source'),
                children:
                  item.source === 'workflow'
                    ? t('source_workflow')
                    : t('source_api')
              },
              {
                key: 'display_name',
                label: t('display_name'),
                children: item.display_name ?? t('deleted_channel')
              },
              {
                key: 'type_code',
                label: t('type'),
                children: item.type_code ?? '—'
              },
              {
                key: 'status',
                label: t('send_status'),
                children: statusLabel(item.status)
              },
              {
                key: 'failure_code',
                label: t('failure_reason'),
                children: failureLabel(item.failure_code)
              },
              {
                key: 'recipient_count',
                label: t('recipient_count'),
                children: item.recipient_count ?? '—'
              },
              {
                key: 'completed_at',
                label: t('completed_at'),
                children: item.completed_at
                  ? formatDateTime(item.completed_at)
                  : '—'
              }
            ]}
          />
          <Typography.Title level={5}>{t('message_title')}</Typography.Title>
          <Typography.Paragraph className="notification-log-detail__text">
            {item.title ?? '—'}
          </Typography.Paragraph>
          <Typography.Title level={5}>{t('message_content')}</Typography.Title>
          <Typography.Paragraph className="notification-log-detail__text">
            {item.content ?? '—'}
          </Typography.Paragraph>
        </>
      )}
    </ResizableDrawer>
  );
}
