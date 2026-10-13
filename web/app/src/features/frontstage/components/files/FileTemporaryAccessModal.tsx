import { useEffect, useRef, useState } from 'react';
import {
  Alert,
  App,
  Button,
  Form,
  Input,
  InputNumber,
  Modal,
  Space,
  Typography
} from 'antd';
import copy from 'copy-to-clipboard';
import {
  createConsoleTemporaryFileLink,
  type TemporaryFileLink
} from '@1flowbase/api-client';
import { i18nText } from '../../../../shared/i18n/text';
import { formatDateTime } from '../../../../shared/i18n/format';
import { useNativeBlockSurface } from '../../lib/native-modules/native-block-surface-context';

interface Props {
  fileTableId: string;
  recordId: string;
  filename: string;
  csrfToken: string;
  onClose: () => void;
}

export function FileTemporaryAccessModal({
  fileTableId,
  recordId,
  filename,
  csrfToken,
  onClose
}: Props) {
  const { message } = App.useApp();
  const surface = useNativeBlockSurface();
  const [form] = Form.useForm<{ expires_in_seconds: number }>();
  const [link, setLink] = useState<TemporaryFileLink>();
  const [error, setError] = useState<string>();
  const [generating, setGenerating] = useState(false);
  const pending = useRef<AbortController | null>(null);
  useEffect(() => () => pending.current?.abort(), []);
  const absoluteUrl = link
    ? new URL(link.url, window.location.origin).href
    : '';

  async function generate(input: { expires_in_seconds: number }) {
    pending.current?.abort();
    const controller = new AbortController();
    pending.current = controller;
    setGenerating(true);
    setError(undefined);
    setLink(undefined);
    try {
      const result = await createConsoleTemporaryFileLink(
        fileTableId,
        recordId,
        input,
        csrfToken,
        controller.signal
      );
      if (!controller.signal.aborted) setLink(result);
    } catch (failure) {
      if (!controller.signal.aborted)
        setError(
          failure instanceof Error
            ? failure.message
            : i18nText('frontstage', 'files.link_failed')
        );
    } finally {
      if (!controller.signal.aborted) setGenerating(false);
    }
  }

  return (
    <Modal
      open
      title={i18nText('frontstage', 'files.temporary_public')}
      getContainer={surface?.overlayHost.getPopupContainer}
      onCancel={onClose}
      footer={
        <Button onClick={onClose}>
          {i18nText('frontstage', 'files.close')}
        </Button>
      }
    >
      <Space orientation="vertical" size={16} style={{ width: '100%' }}>
        <Typography.Text>{filename}</Typography.Text>
        <Typography.Paragraph type="secondary" style={{ margin: 0 }}>
          {i18nText('frontstage', 'files.temporary_public_hint')}
        </Typography.Paragraph>
        <Form
          form={form}
          layout="vertical"
          initialValues={{ expires_in_seconds: 600 }}
          onFinish={(values) => void generate(values)}
          onValuesChange={() => setLink(undefined)}
        >
          <Form.Item
            name="expires_in_seconds"
            label={i18nText('frontstage', 'files.expires_in_seconds')}
            rules={[
              {
                validator: (_, value: number | null) =>
                  value != null && Number.isSafeInteger(value) && value > 0
                    ? Promise.resolve()
                    : Promise.reject(
                        new Error(
                          i18nText('frontstage', 'files.positive_duration')
                        )
                      )
              }
            ]}
          >
            <InputNumber
              disabled={generating}
              min={1}
              step={60}
              style={{ width: '100%' }}
            />
          </Form.Item>
          <Button type="primary" htmlType="submit" loading={generating}>
            {i18nText('frontstage', 'files.generate_link')}
          </Button>
        </Form>
        {error ? <Alert showIcon type="error" title={error} /> : null}
        {link ? (
          <Space orientation="vertical" style={{ width: '100%' }}>
            <Input
              aria-label={i18nText('frontstage', 'files.temporary_url')}
              readOnly
              value={absoluteUrl}
            />
            {link.preview_url ? (
              <Typography.Link
                href={new URL(link.preview_url, window.location.origin).href}
                target="_blank"
                rel="noreferrer"
              >
                {i18nText('frontstage', 'files.open_public_preview')}
              </Typography.Link>
            ) : null}
            <Typography.Text type="secondary">
              {i18nText('frontstage', 'files.expires_at', {
                time: formatDateTime(link.expires_at)
              })}
            </Typography.Text>
            <Button
              onClick={async () => {
                try {
                  if (await copy(absoluteUrl))
                    message.success(
                      i18nText('frontstage', 'files.link_copied')
                    );
                  else
                    message.error(i18nText('frontstage', 'files.copy_failed'));
                } catch {
                  message.error(i18nText('frontstage', 'files.copy_failed'));
                }
              }}
            >
              {i18nText('frontstage', 'files.copy_link')}
            </Button>
          </Space>
        ) : null}
      </Space>
    </Modal>
  );
}
