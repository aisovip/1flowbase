import { useEffect, useRef, useState } from 'react';
import { App, Button, Drawer, Form, Input } from 'antd';
import {
  deleteConsoleRuntimeModelRecord,
  updateConsoleRuntimeModelRecord,
  type ConsoleFileRecord
} from '@1flowbase/api-client';
import { i18nText } from '../../../../shared/i18n/text';
import { useNativeBlockSurface } from '../../lib/native-modules/native-block-surface-context';

export function FileRecordActions({
  record,
  fileTableCode,
  csrfToken,
  onShare,
  onChanged
}: {
  record: ConsoleFileRecord;
  fileTableCode: string;
  csrfToken?: string | null;
  onShare: () => void;
  onChanged: () => Promise<void>;
}) {
  const { message, modal } = App.useApp();
  const surface = useNativeBlockSurface();
  const confirmation = useRef<{ destroy: () => void } | null>(null);
  useEffect(() => () => confirmation.current?.destroy(), []);
  const [editing, setEditing] = useState(false);
  const [saving, setSaving] = useState(false);
  const [form] = Form.useForm<{ filename: string }>();
  return (
    <>
      <div className="file-table-view__actions">
        <Button
          className="file-table-view__share"
          type="text"
          disabled={!csrfToken}
          onClick={onShare}
        >
          {i18nText('frontstage', 'files.share')}
        </Button>
        <Button
          type="text"
          disabled={!csrfToken}
          onClick={() => {
            form.setFieldsValue({ filename: record.filename });
            setEditing(true);
          }}
        >
          {i18nText('frontstage', 'auto.edit')}
        </Button>
        <Button
          danger
          type="text"
          disabled={!csrfToken}
          onClick={() => {
            confirmation.current = modal.confirm({
              title: i18nText('frontstage', 'files.delete_title'),
              content: i18nText('frontstage', 'files.delete_description', {
                filename: record.filename
              }),
              okText: i18nText('frontstage', 'auto.delete'),
              cancelText: i18nText('frontstage', 'auto.cancel'),
              okButtonProps: {
                danger: true,
                'aria-label': i18nText('frontstage', 'auto.delete')
              },
              getContainer: surface?.overlayHost.getPopupContainer,
              onOk: async () => {
                if (!csrfToken) return;
                try {
                  await deleteConsoleRuntimeModelRecord(
                    fileTableCode,
                    record.id,
                    csrfToken
                  );
                  await onChanged();
                  message.success(i18nText('frontstage', 'files.deleted'));
                } catch (error) {
                  message.error((error as Error).message);
                  throw error;
                }
              }
            });
          }}
        >
          {i18nText('frontstage', 'auto.delete')}
        </Button>
      </div>
      <Drawer
        open={editing}
        title={i18nText('frontstage', 'files.edit_title')}
        size={420}
        getContainer={surface?.overlayHost.getPopupContainer}
        destroyOnHidden
        onClose={() => {
          if (!saving) setEditing(false);
        }}
        extra={
          <Button
            type="primary"
            aria-label={i18nText('frontstage', 'files.save')}
            loading={saving}
            onClick={() => form.submit()}
          >
            {i18nText('frontstage', 'files.save')}
          </Button>
        }
      >
        <Form
          form={form}
          layout="vertical"
          onFinish={async ({ filename }) => {
            if (!csrfToken) return;
            setSaving(true);
            try {
              await updateConsoleRuntimeModelRecord(
                fileTableCode,
                record.id,
                { filename: filename.trim() },
                csrfToken
              );
              await onChanged();
              setEditing(false);
              message.success(i18nText('frontstage', 'files.saved'));
            } catch (error) {
              message.error((error as Error).message);
            } finally {
              setSaving(false);
            }
          }}
        >
          <Form.Item
            name="filename"
            label={i18nText('frontstage', 'files.filename')}
            rules={[
              {
                required: true,
                whitespace: true,
                message: i18nText('frontstage', 'files.filename_required')
              }
            ]}
          >
            <Input autoFocus />
          </Form.Item>
        </Form>
      </Drawer>
    </>
  );
}
