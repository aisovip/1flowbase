import { useMemo, useState } from 'react';
import { Alert, Form, Input, Space } from 'antd';
import { useTranslation } from 'react-i18next';
import type {
  NotificationInstance,
  NotificationType
} from '@1flowbase/api-client';
import {
  SchemaFormDrawer,
  type SchemaFormValues
} from '../../../../shared/schema-ui/v1/form-drawer/SchemaFormDrawer';
import type { PluginFormSchema } from '../../../../shared/schema-ui/v1/contracts/plugin-form-schema';
import {
  createNotificationInstance,
  updateNotificationInstance
} from '../../../notifications/api/queries';
import { NotificationAuthorization } from './NotificationAuthorization';
import { useAuthStore } from '../../../../state/auth-store';

export function NotificationInstanceDrawer({
  instance,
  type,
  onClose,
  onSaved
}: {
  instance?: NotificationInstance;
  type?: NotificationType;
  onClose: () => void;
  onSaved: (options?: { keepOpen: boolean }) => void;
}) {
  const { t } = useTranslation('notifications');
  const csrfToken = useAuthStore((state) => state.csrfToken) ?? '';
  const [savedInstance, setSavedInstance] = useState(instance);
  const [beginOnMount, setBeginOnMount] = useState(false);
  const [display_name, setDisplayName] = useState(instance?.display_name ?? '');
  const [nameError, setNameError] = useState(false);
  const schema = useMemo<PluginFormSchema>(() => {
    if (!type) return { schema_version: '1.0.0', fields: [] };
    // Canonical wire schema is the same PluginFormSchema consumed by schema UI.
    const form = type.config_form as PluginFormSchema;
    return {
      ...form,
      fields: form.fields.map((field) =>
        savedInstance?.configured_secret_fields.includes(field.key)
          ? {
              ...field,
              required: false,
              default_value: undefined,
              placeholder: t('secret_preserved')
            }
          : field
      )
    };
  }, [savedInstance, type, t]);
  const initialValues = useMemo(
    () => savedInstance?.config_json as SchemaFormValues | undefined,
    [savedInstance]
  );
  const unavailable = !!savedInstance && !savedInstance.available;
  const unavailableTitle =
    savedInstance?.unavailable_reason === 'instance_disabled'
      ? t('instance_disabled')
      : savedInstance?.unavailable_reason === 'plugin_unavailable'
        ? t('plugin_unavailable')
        : t('type_unavailable');
  return (
    <SchemaFormDrawer
      open
      title={savedInstance ? t('edit_instance') : t('create_instance')}
      rootClassName="notification-instance-drawer"
      footerAlign="end"
      schema={schema}
      initialValues={initialValues}
      resizable
      defaultWidth={720}
      minWidth={480}
      maxWidth={1200}
      resizeLabel={t('resize_drawer')}
      submitText={
        !savedInstance && type?.authorization
          ? t('save_and_authorize')
          : t('submit')
      }
      cancelText={t('cancel')}
      leadingContent={
        <Space orientation="vertical" size="middle" style={{ width: '100%' }}>
          <Form layout="vertical" component={false}>
            <Form.Item
              label={t('display_name')}
              required
              validateStatus={nameError ? 'error' : undefined}
              help={nameError ? t('name_required') : undefined}
            >
              <Input
                autoFocus
                value={display_name}
                onChange={(event) => {
                  setDisplayName(event.target.value);
                  setNameError(false);
                }}
                aria-label={t('display_name')}
                autoComplete="off"
              />
            </Form.Item>
            <Form.Item label={t('type')}>
              <Input
                readOnly
                value={type?.display_name ?? instance?.type_code}
                aria-label={t('type')}
              />
            </Form.Item>
          </Form>
          {type?.authorization && savedInstance?.authorization && (
            <NotificationAuthorization
              instance_id={savedInstance.id}
              descriptor={type.authorization}
              authorization={savedInstance.authorization}
              beginOnMount={beginOnMount}
              onChanged={() => onSaved({ keepOpen: true })}
            />
          )}
          {unavailable && <Alert type="warning" title={unavailableTitle} />}
          {!type && instance && (
            <Alert type="info" title={t('config_unavailable')} />
          )}
        </Space>
      }
      submitDisabled={
        !display_name.trim() || (!savedInstance && !type?.available)
      }
      onCancel={onClose}
      onBeforeSubmit={() => {
        if (!display_name.trim()) {
          setNameError(true);
          return false;
        }
      }}
      onSubmit={async (values) => {
        try {
          const config_json = Object.fromEntries(
            Object.entries(values).filter(
              ([key, value]) =>
                value !== undefined &&
                !(
                  savedInstance?.configured_secret_fields.includes(key) &&
                  (value === '' || value === null)
                )
            )
          );
          if (savedInstance) {
            return await updateNotificationInstance(
              savedInstance.id,
              {
                display_name: display_name.trim(),
                ...(type ? { config_json } : {})
              },
              csrfToken
            );
          } else if (type?.available) {
            return await createNotificationInstance(
              {
                display_name: display_name.trim(),
                type_code: type.type_code,
                installation_id: type.installation_id,
                config_json
              },
              csrfToken
            );
          }
        } catch {
          throw new Error(t('instances_failed'));
        }
      }}
      onSubmitSuccess={(result) => {
        if (type?.authorization) {
          setBeginOnMount(!savedInstance);
          setSavedInstance(result as NotificationInstance);
          onSaved({ keepOpen: true });
        } else {
          onSaved();
        }
      }}
    />
  );
}
