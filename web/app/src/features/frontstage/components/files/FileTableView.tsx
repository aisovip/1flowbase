import { useContext, useEffect, useMemo, useRef, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import {
  Alert,
  App,
  Button,
  ConfigProvider,
  Image,
  Modal,
  Space,
  Typography,
  Upload
} from 'antd';
import type { UploadFile, UploadProps } from 'antd';
import InboxOutlined from '@ant-design/icons/es/icons/InboxOutlined';
import UploadOutlined from '@ant-design/icons/es/icons/UploadOutlined';
import ReloadOutlined from '@ant-design/icons/es/icons/ReloadOutlined';
import {
  fetchConsoleFileRecords,
  fetchConsoleFileTables,
  uploadConsoleFile,
  type ConsoleFileRecord
} from '@1flowbase/api-client';
import { useAuthStore } from '../../../../state/auth-store';
import { i18nText } from '../../../../shared/i18n/text';
import { formatDateTime, formatNumber } from '../../../../shared/i18n/format';
import {
  DataTable,
  DataTableColumnSettings,
  type DataTableColumn
} from '../../../../shared/ui/data-table/DataTable';
import { DataTableLayout } from '../../../../shared/ui/data-table/DataTableLayout';
import { usePersistedDataTableConfiguration } from '../../../../shared/ui/data-table/data-table-state';
import { useNativeBlockSurface } from '../../lib/native-modules/native-block-surface-context';
import { FileTemporaryAccessModal } from './FileTemporaryAccessModal';

export interface FileTableViewProps {
  fileTableCode: string;
}

/** Host owns authenticated multipart requests; Blocks only select a file table. */
export function FileTableView({ fileTableCode }: FileTableViewProps) {
  const { message } = App.useApp();
  const { getPrefixCls } = useContext(ConfigProvider.ConfigContext);
  const uploadPrefixCls = getPrefixCls('upload');
  const queryClient = useQueryClient();
  const csrfToken = useAuthStore((state) => state.csrfToken);
  const workspaceId = useAuthStore(
    (state) => state.actor?.current_workspace_id
  );
  const surface = useNativeBlockSurface();
  const [page, setPage] = useState(1);
  const [uploadOpen, setUploadOpen] = useState(false);
  const [fileList, setFileList] = useState<UploadFile[]>([]);
  const [publicTarget, setPublicTarget] = useState<{
    record: ConsoleFileRecord;
    tableId: string;
    workspaceId: string | undefined;
  }>();
  const uploads = useRef(new Set<AbortController>());
  useEffect(() => {
    const controllers = uploads.current;
    return () => {
      for (const controller of controllers) controller.abort();
    };
  }, [workspaceId]);

  const tables = useQuery({
    queryKey: ['files', 'tables', workspaceId],
    queryFn: () => fetchConsoleFileTables()
  });
  const table = tables.data?.find((item) => item.code === fileTableCode);
  const pageSize = 20;
  const records = useQuery({
    queryKey: ['files', 'records', workspaceId, table?.id, page],
    queryFn: ({ signal }) =>
      fetchConsoleFileRecords(fileTableCode, page, pageSize, signal),
    enabled: Boolean(table)
  });
  useEffect(() => {
    setPage(1);
    setPublicTarget(undefined);
  }, [fileTableCode, workspaceId]);
  useEffect(() => setPublicTarget(undefined), [csrfToken]);

  const columns = useMemo<DataTableColumn<ConsoleFileRecord>[]>(
    () => [
      {
        key: 'preview_url',
        dataIndex: 'preview_url',
        title: i18nText('frontstage', 'files.image_preview'),
        width: 112,
        render: (_, record) =>
          record.preview_url ? (
            <Image
              src={record.preview_url}
              alt={record.filename}
              width={64}
              height={64}
              loading="lazy"
              style={{ objectFit: 'contain' }}
              preview={{ getContainer: surface?.overlayHost.getPopupContainer }}
            />
          ) : null
      },
      {
        key: 'filename',
        dataIndex: 'filename',
        title: i18nText('frontstage', 'files.filename'),
        width: 280,
        render: (_, record) =>
          table ? (
            <a href={record.url} target="_blank" rel="noreferrer">
              {record.filename}
            </a>
          ) : (
            record.filename
          )
      },
      {
        key: 'size',
        dataIndex: 'size',
        title: i18nText('frontstage', 'files.size'),
        width: 140,
        render: (_, record) => `${formatNumber(record.size)} B`
      },
      {
        key: 'mimetype',
        dataIndex: 'mimetype',
        title: i18nText('frontstage', 'files.mimetype'),
        width: 220
      },
      {
        key: 'url',
        dataIndex: 'url',
        title: i18nText('frontstage', 'files.url'),
        width: 360,
        render: (_, record) => (
          <a href={record.url} target="_blank" rel="noreferrer">
            {record.url}
          </a>
        )
      },
      {
        key: 'actions',
        title: i18nText('frontstage', 'files.actions'),
        width: 140,
        render: (_, record) => (
          <Button
            type="link"
            disabled={!table || !csrfToken}
            onClick={() =>
              table &&
              setPublicTarget({ record, tableId: table.id, workspaceId })
            }
          >
            {i18nText('frontstage', 'files.temporary_public')}
          </Button>
        )
      },
      {
        key: 'created_at',
        dataIndex: 'created_at',
        title: i18nText('frontstage', 'files.created_at'),
        width: 200,
        render: (_, record) => formatDateTime(record.created_at)
      }
    ],
    [table, csrfToken, workspaceId, surface]
  );
  const configuration = usePersistedDataTableConfiguration({
    columns,
    storageKey: `file-table:${workspaceId}:${fileTableCode}`
  });
  const uploading = fileList.some((file) => file.status === 'uploading');

  const customRequest: UploadProps['customRequest'] = (request) => {
    const controller = new AbortController();
    uploads.current.add(controller);
    async function upload() {
      if (!table || !csrfToken || !(request.file instanceof File)) {
        throw new Error(i18nText('frontstage', 'files.upload_unavailable'));
      }
      const result = await uploadConsoleFile(
        request.file,
        table.id,
        csrfToken,
        controller.signal
      );
      if (controller.signal.aborted) return;
      request.onSuccess?.(result);
      message.success(
        i18nText('frontstage', 'files.upload_success', {
          filename: request.file.name
        })
      );
      // Invalidate cached pages before returning to the newest records.
      await queryClient.invalidateQueries({
        queryKey: ['files', 'records', workspaceId, table.id]
      });
      setPage(1);
    }
    void upload()
      .catch((error: unknown) => {
        if (controller.signal.aborted) return;
        const failure =
          error instanceof Error
            ? error
            : new Error(i18nText('frontstage', 'files.upload_failed'));
        request.onError?.(failure);
        message.error(failure.message);
      })
      .finally(() => {
        uploads.current.delete(controller);
      });
    return { abort: () => controller.abort() };
  };

  const error = tables.error ?? records.error;
  return (
    <Space orientation="vertical" size={16} style={{ width: '100%' }}>
      <Typography.Title level={4} style={{ margin: 0 }}>
        {table?.title ?? fileTableCode}
      </Typography.Title>
      {error ? <Alert showIcon type="error" title={error.message} /> : null}
      {!tables.isPending && !tables.error && !table ? (
        <Alert
          showIcon
          type="warning"
          title={i18nText('frontstage', 'files.table_unavailable')}
        />
      ) : null}
      <DataTableLayout>
        <DataTable
          columns={columns}
          configuration={configuration}
          dataSource={records.data?.items ?? []}
          rowKey="id"
          loading={tables.isPending || records.isFetching}
          page={page}
          pageSize={pageSize}
          total={records.data?.total ?? 0}
          onPageChange={setPage}
          emptyText={i18nText('frontstage', 'files.empty')}
          toolbar={
            <Space wrap>
              <Button
                icon={<ReloadOutlined />}
                disabled={!table}
                onClick={() => void records.refetch()}
              >
                {i18nText('frontstage', 'files.refresh')}
              </Button>
              <DataTableColumnSettings
                columns={columns}
                configuration={configuration}
              />
              <Button
                type="primary"
                icon={<UploadOutlined />}
                disabled={!table || !csrfToken}
                onClick={() => setUploadOpen(true)}
              >
                {i18nText('frontstage', 'files.upload')}
              </Button>
            </Space>
          }
        />
      </DataTableLayout>
      {publicTarget &&
      publicTarget.tableId === table?.id &&
      publicTarget.workspaceId === workspaceId &&
      csrfToken ? (
        <FileTemporaryAccessModal
          key={`${publicTarget.tableId}:${publicTarget.record.id}:${workspaceId}`}
          fileTableId={publicTarget.tableId}
          recordId={publicTarget.record.id}
          filename={publicTarget.record.filename}
          csrfToken={csrfToken}
          onClose={() => setPublicTarget(undefined)}
        />
      ) : null}
      <Modal
        open={uploadOpen}
        title={i18nText('frontstage', 'files.upload')}
        getContainer={surface?.overlayHost.getPopupContainer}
        destroyOnHidden
        mask={{ closable: !uploading }}
        closable={!uploading}
        onCancel={() => {
          if (!uploading) {
            setUploadOpen(false);
            setFileList([]);
          }
        }}
        footer={
          <Button
            disabled={uploading}
            onClick={() => {
              setUploadOpen(false);
              setFileList([]);
            }}
          >
            {i18nText('frontstage', 'files.close')}
          </Button>
        }
      >
        <Upload.Dragger
          multiple
          fileList={fileList}
          customRequest={customRequest}
          onChange={({ fileList: next }) => setFileList(next)}
        >
          <p className={`${uploadPrefixCls}-drag-icon`}>
            <InboxOutlined />
          </p>
          <p className={`${uploadPrefixCls}-text`}>
            {i18nText('frontstage', 'files.upload_prompt')}
          </p>
          <p className={`${uploadPrefixCls}-hint`}>
            {i18nText('frontstage', 'files.upload_hint')}
          </p>
        </Upload.Dragger>
      </Modal>
    </Space>
  );
}
