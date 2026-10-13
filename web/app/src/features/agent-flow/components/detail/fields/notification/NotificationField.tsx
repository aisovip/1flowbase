import { useQuery } from '@tanstack/react-query';
import { Alert, Select, Space, Spin, Typography } from 'antd';
import { useTranslation } from 'react-i18next';
import type {
  FlowBinding,
  FlowNodeDocument,
  NamedBindingEntry,
  NamedBindingExpression
} from '@1flowbase/flow-schema';
import type { SchemaFieldRendererProps } from '../../../../../../shared/schema-ui/v1/registry/create-renderer-registry';
import type { PluginFormFieldSchema } from '../../../../../../shared/schema-ui/v1/contracts/plugin-form-schema';
import { useAuthStore } from '../../../../../../state/auth-store';
import { selectNavigationQueryScope } from '../../../../../../state/navigation-query-scope';
import {
  notificationInstancesOptions,
  notificationTypesOptions
} from '../../../../../notifications/api/queries';
import { getNamedBindingExpression } from '../../../../lib/named-binding-expressions';
import type { FlowSelectorOption } from '../../../../lib/selector-options';
import { NotificationInputField } from './NotificationInputField';

export function NotificationField({ adapter }: SchemaFieldRendererProps) {
  const { t } = useTranslation('agentFlow');
  const scope = useAuthStore(selectNavigationQueryScope);
  const instances = useQuery(notificationInstancesOptions(scope));
  const types = useQuery(notificationTypesOptions(scope));
  const node = adapter.getDerived('node') as FlowNodeDocument;
  const instance_id =
    typeof node.config.instance_id === 'string' ? node.config.instance_id : '';
  const instance = instances.data?.find((item) => item.id === instance_id);
  const type = types.data?.find(
    (item) =>
      item.type_code === instance?.type_code &&
      item.installation_id === instance?.installation_id
  );
  const binding = adapter.getValue('bindings.input') as FlowBinding | undefined;
  const entries = binding?.kind === 'named_bindings' ? binding.value : [];
  const options = adapter.getDerived('selectorOptions') as FlowSelectorOption[];
  const pending = instances.isPending || types.isPending;
  const failed = instances.isError || types.isError;
  function updateEntry(
    name: string,
    valueType: string,
    expression?: NamedBindingExpression
  ) {
    const next: NamedBindingEntry[] = entries.filter(
      (entry) => entry.name !== name
    );
    if (expression) next.push({ name, valueType, value: expression });
    adapter.setValue('bindings.input', { kind: 'named_bindings', value: next });
  }
  function availability() {
    if (!instance_id || pending || failed) return null;
    if (!instance) return t('auto.notification_missing_instance');
    if (instance.status === 'disabled')
      return t('auto.notification_instance_disabled');
    if (!instance.available || !type?.available)
      return t('auto.notification_instance_unavailable');
    return null;
  }
  return (
    <Space orientation="vertical" size="middle" style={{ width: '100%' }}>
      <Select
        aria-label={t('auto.notification_instance')}
        style={{ width: '100%' }}
        loading={pending}
        value={instance_id || undefined}
        showSearch
        optionFilterProp="label"
        options={(instances.data ?? []).map((item) => ({
          value: item.id,
          label: `${item.display_name}${item.status === 'disabled' ? ` · ${t('auto.notification_instance_disabled')}` : !item.available ? ` · ${t('auto.notification_instance_unavailable')}` : ''}`,
          disabled: item.status === 'disabled' || !item.available
        }))}
        onChange={(id) => {
          const selected = instances.data?.find((item) => item.id === id);
          const descriptor = types.data?.find(
            (item) =>
              item.type_code === selected?.type_code &&
              item.installation_id === selected?.installation_id
          );
          if (descriptor) {
            const properties = descriptor.input_schema.properties as
              | Record<string, { type?: string }>
              | undefined;
            const input: NamedBindingEntry[] = descriptor.input_form.fields
              .filter((field) => field.default_value !== undefined)
              .map((field) => ({
                name: field.key,
                valueType:
                  properties?.[field.key]?.type === 'integer'
                    ? 'number'
                    : (properties?.[field.key]?.type ?? field.type),
                value: { kind: 'constant', value: field.default_value }
              }));
            adapter.setValue('config.instance_id', {
              instance_id: id,
              output_schema: descriptor.output_schema,
              input
            });
          }
        }}
      />
      {pending && <Spin />}
      {failed && (
        <Alert type="error" title={t('auto.notification_options_failed')} />
      )}
      {availability() && <Alert type="warning" title={availability()} />}
      {type && (
        <>
          {type.description && (
            <Typography.Text type="secondary">
              {type.description}
            </Typography.Text>
          )}
          {type.input_form.fields.map((rawField) => {
            const field = rawField as PluginFormFieldSchema;
            const properties = type.input_schema.properties as
              | Record<string, { type?: string }>
              | undefined;
            const schemaType = properties?.[field.key]?.type ?? field.type;
            const valueType = schemaType === 'integer' ? 'number' : schemaType;
            const entry = entries.find((item) => item.name === field.key);
            return (
              <NotificationInputField
                key={`${instance_id}:${field.key}`}
                field={field}
                valueType={valueType}
                expression={
                  entry?.value ??
                  (entry ? getNamedBindingExpression(entry) : null)
                }
                options={options ?? []}
                onClear={() => updateEntry(field.key, valueType)}
                onChange={(expression) =>
                  updateEntry(field.key, valueType, expression)
                }
              />
            );
          })}
        </>
      )}
    </Space>
  );
}
