import { expect, test } from 'vitest';
import type { NotificationType } from '@1flowbase/api-client';
import { localizeNotificationType } from '../lib/type-i18n';

const field = {
  key: 'text',
  label: 'Canonical label',
  type: 'string',
  default_value: 'original payload',
  required: true,
  options: [{ label: 'User supplied label', value: 'original-value' }],
  visible_when: [],
  disabled_when: []
};
const type: NotificationType = {
  type_code: 'vendor/notice/private',
  installation_id: 'plugin-installation',
  display_name: 'Canonical name',
  description: 'Canonical description',
  contribution_id: 'vendor.notice',
  available: true,
  unavailable_reason: null,
  config_schema: { type: 'object', properties: { text: { type: 'string' } } },
  input_schema: { type: 'object', properties: { text: { type: 'string' } } },
  output_schema: { type: 'object' },
  config_form: { schema_version: '1.0.0', fields: [field] },
  input_form: { schema_version: '1.0.0', fields: [field] },
  authorization: {
    actions: [{ code: 'qr', kind: 'qr_code', label: 'Canonical scan' }],
    managed_secret_keys: ['token']
  },
  i18n: {
    en_US: {
      display_name: 'Plugin notice',
      description: 'English plugin description',
      config_form: {
        fields: {
          text: { label: 'English config', placeholder: 'English placeholder' }
        }
      },
      input_form: {
        fields: {
          text: { label: 'English input', description: 'English field help' }
        }
      },
      authorization: { actions: { qr: { label: 'Plugin scan' } } }
    },
    zh_Hans: {
      display_name: '插件通知',
      config_form: { fields: { text: { label: '配置正文' } } },
      input_form: { fields: { text: { label: '通知正文' } } },
      authorization: { actions: { qr: { label: '插件扫码' } } }
    }
  }
};

test.each(['zh_Hans', 'zh-CN', 'ZH-hans', 'zh'])(
  'normalizes %s and falls back per missing presentation field',
  (locale) => {
    const result = localizeNotificationType(type, locale);
    expect(result.display_name).toBe('插件通知');
    expect(result.description).toBe('English plugin description');
    expect(result.config_form.fields[0]).toMatchObject({
      label: '配置正文',
      placeholder: 'English placeholder'
    });
    expect(result.input_form.fields[0]).toMatchObject({
      label: '通知正文',
      description: 'English field help'
    });
    expect(result.authorization?.actions[0]).toMatchObject({
      code: 'qr',
      label: '插件扫码'
    });
  }
);

test.each(['en_US', 'en-US', 'en', 'fr_FR'])(
  'uses English fallback for %s',
  (locale) => {
    expect(localizeNotificationType(type, locale).display_name).toBe(
      'Plugin notice'
    );
  }
);

test('localizes only presentation without mutating the raw catalog, schemas, binding identity or values', () => {
  const before = structuredClone(type);
  const result = localizeNotificationType(type, 'zh_Hans');
  expect(type).toEqual(before);
  expect(result.config_schema).toBe(type.config_schema);
  expect(result.input_schema).toBe(type.input_schema);
  expect(result.output_schema).toBe(type.output_schema);
  expect(result.input_form.fields[0]).toMatchObject({
    key: 'text',
    default_value: 'original payload',
    required: true
  });
  expect(result.input_form.fields[0].options).toBe(
    type.input_form.fields[0].options
  );
  expect(result.authorization?.managed_secret_keys).toBe(
    type.authorization?.managed_secret_keys
  );
  expect(result.type_code).toBe(type.type_code);
  expect(result.installation_id).toBe(type.installation_id);
});

test('preserves legacy plugin metadata and uses host-provided builtin text only for the builtin system type', () => {
  const legacy = { ...type, i18n: undefined };
  expect(localizeNotificationType(legacy, 'zh_Hans')).toBe(legacy);
  expect(
    localizeNotificationType(
      { ...type, type_code: 'system', installation_id: null },
      'zh_Hans',
      {
        display_name: '系统通知',
        input_form: { fields: { text: { label: '系统字段' } } }
      }
    ).display_name
  ).toBe('系统通知');
  expect(
    localizeNotificationType(type, 'zh_Hans', { display_name: '系统通知' })
      .display_name
  ).toBe('插件通知');
});
