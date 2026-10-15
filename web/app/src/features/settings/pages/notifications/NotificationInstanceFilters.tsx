import FilterOutlined from '@ant-design/icons/es/icons/FilterOutlined';
import { Button, Flex, Form, Input, Popover, Select } from 'antd';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type {
  NotificationInstanceListInput,
  NotificationType
} from '@1flowbase/api-client';
import { useNotificationTypeLocalization } from '../../../notifications/hooks/use-notification-type-localization';

export function NotificationInstanceFilters({
  types,
  value,
  onChange
}: {
  types: NotificationType[];
  value: NotificationInstanceListInput;
  onChange: (value: NotificationInstanceListInput) => void;
}) {
  const { t } = useTranslation('notifications');
  const localizeType = useNotificationTypeLocalization();
  const [open, setOpen] = useState(false);
  const [form] = Form.useForm<NotificationInstanceListInput>();
  const count = Object.values(value).filter(Boolean).length;
  const typeOptions = Array.from(
    new Map(
      types.map((type) => [
        type.type_code,
        {
          value: type.type_code,
          label: localizeType(type).display_name
        }
      ])
    ).values()
  );
  return (
    <Popover
      trigger="click"
      placement="bottomLeft"
      open={open}
      onOpenChange={(next) => {
        if (next) {
          form.resetFields();
          form.setFieldsValue(value);
        }
        setOpen(next);
      }}
      content={
        <Form<NotificationInstanceListInput>
          form={form}
          className="notifications-page__filters"
          layout="vertical"
          onFinish={(values) => {
            onChange({
              ...(values.display_name?.trim()
                ? { display_name: values.display_name.trim() }
                : {}),
              ...(values.type_code ? { type_code: values.type_code } : {}),
              ...(values.status ? { status: values.status } : {})
            });
            setOpen(false);
          }}
        >
          <Form.Item name="display_name" label={t('display_name')}>
            <Input allowClear />
          </Form.Item>
          <Form.Item name="type_code" label={t('type')}>
            <Select allowClear options={typeOptions} />
          </Form.Item>
          <Form.Item name="status" label={t('status')}>
            <Select
              allowClear
              options={[
                { value: 'enabled', label: t('enabled') },
                { value: 'disabled', label: t('disabled') }
              ]}
            />
          </Form.Item>
          <Flex justify="flex-end" gap="small">
            <Button
              onClick={() => {
                form.resetFields();
                onChange({});
                setOpen(false);
              }}
            >
              {t('clear_filters')}
            </Button>
            <Button type="primary" htmlType="submit">
              {t('apply_filters')}
            </Button>
          </Flex>
        </Form>
      }
    >
      <Button icon={<FilterOutlined />} type={count ? 'primary' : 'default'}>
        {t('filter')}
        {count ? ` (${count})` : ''}
      </Button>
    </Popover>
  );
}
