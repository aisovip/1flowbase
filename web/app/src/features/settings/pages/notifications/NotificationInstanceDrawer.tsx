import { useMemo, useState } from 'react';
import { Alert, Input, Select, Space, Typography } from 'antd';
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
import { useAuthStore } from '../../../../state/auth-store';

export function NotificationInstanceDrawer({
  instance,
  types,
  onClose,
  onSaved
}: {
  instance?: NotificationInstance;
  types: NotificationType[];
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation('notifications');
  const csrfToken = useAuthStore((state) => state.csrfToken) ?? '';
  const [display_name, setDisplayName] = useState(instance?.display_name ?? '');
  const [selected, setSelected] = useState<NotificationType | undefined>(() =>
    types.find(
      (type) =>
        type.type_code === instance?.type_code &&
        type.installation_id === instance?.installation_id
    )
  );
  const [nameError, setNameError] = useState(false);
  const schema = useMemo<PluginFormSchema>(() => {
    if (!selected) return { schema_version: '1.0.0', fields: [] };
    // Canonical wire schema is the same PluginFormSchema consumed by schema UI.
    const form = selected.config_form as PluginFormSchema;
    return {
      ...form,
      fields: form.fields.map((field) =>
        instance?.configured_secret_fields.includes(field.key)
          ? {
              ...field,
              required: false,
              default_value: undefined,
              placeholder: t('secret_preserved')
            }
          : field
      )
    };
  }, [instance, selected, t]);
  const initialValues = useMemo(
    () => instance?.config_json as SchemaFormValues | undefined,
    [instance]
  );
  const unavailable = !!instance && !instance.available;
  return (
    <SchemaFormDrawer
      key={JSON.stringify([selected?.type_code, selected?.installation_id])}
      open
      title={instance ? t('edit_instance') : t('create_instance')}
      schema={schema}
      initialValues={initialValues}
      width="min(560px, 100vw)"
      leadingContent={
        <Space orientation="vertical" size="middle" style={{ width: '100%' }}>
          <label>
            <Typography.Text>{t('display_name')}</Typography.Text>
            <Input
              value={display_name}
              onChange={(event) => {
                setDisplayName(event.target.value);
                setNameError(false);
              }}
              aria-label={t('display_name')}
              status={nameError ? 'error' : undefined}
            />
            {nameError && (
              <Typography.Text type="danger">
                {t('name_required')}
              </Typography.Text>
            )}
          </label>
          <label>
            <Typography.Text>{t('type')}</Typography.Text>
            <Select
              aria-label={t('type')}
              disabled={!!instance}
              style={{ width: '100%' }}
              value={
                selected
                  ? JSON.stringify([
                      selected.type_code,
                      selected.installation_id
                    ])
                  : undefined
              }
              options={types
                .filter((type) => instance || type.available)
                .map((type) => ({
                  label: type.display_name,
                  value: JSON.stringify([type.type_code, type.installation_id])
                }))}
              onChange={(value) => {
                const [type_code, installation_id] = JSON.parse(value) as [
                  string,
                  string | null
                ];
                setSelected(
                  types.find(
                    (type) =>
                      type.type_code === type_code &&
                      type.installation_id === installation_id
                  )
                );
              }}
            />
          </label>
          {unavailable && (
            <Alert type="warning" title={t('type_unavailable')} />
          )}
          {!selected && instance && (
            <Alert type="info" title={t('config_unavailable')} />
          )}
        </Space>
      }
      submitDisabled={!display_name.trim() || (!instance && !selected)}
      onCancel={onClose}
      onBeforeSubmit={() => {
        if (!display_name.trim()) {
          setNameError(true);
          return false;
        }
      }}
      onSubmit={async (values) => {
        const config_json = Object.fromEntries(
          Object.entries(values).filter(
            ([key, value]) =>
              value !== undefined &&
              !(
                instance?.configured_secret_fields.includes(key) &&
                (value === '' || value === null)
              )
          )
        );
        if (instance) {
          await updateNotificationInstance(
            instance.id,
            {
              display_name: display_name.trim(),
              ...(selected ? { config_json } : {})
            },
            csrfToken
          );
        } else if (selected) {
          await createNotificationInstance(
            {
              display_name: display_name.trim(),
              type_code: selected.type_code,
              installation_id: selected.installation_id,
              config_json
            },
            csrfToken
          );
        }
      }}
      onSubmitSuccess={onSaved}
    />
  );
}
