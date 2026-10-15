import type {
  ConsolePluginFormSchema,
  NotificationFormLocale,
  NotificationType,
  NotificationTypeLocale
} from '@1flowbase/api-client';
import { resolveAppLocale } from '../../../shared/i18n/locales';

function localizeForm(
  form: ConsolePluginFormSchema,
  locale: NotificationFormLocale | undefined
): ConsolePluginFormSchema {
  if (!locale) return form;
  return {
    ...form,
    title: locale.title ?? form.title,
    description: locale.description ?? form.description,
    fields: form.fields.map((field) => {
      const text = locale.fields?.[field.key];
      if (!text) return field;
      return {
        ...field,
        label: text.label ?? field.label,
        description: text.description ?? field.description,
        placeholder: text.placeholder ?? field.placeholder
      };
    })
  };
}

function project(type: NotificationType, locale: NotificationTypeLocale) {
  return {
    ...type,
    display_name: locale.display_name ?? type.display_name,
    description: locale.description ?? type.description,
    config_form: localizeForm(type.config_form, locale.config_form),
    input_form: localizeForm(type.input_form, locale.input_form),
    authorization: type.authorization
      ? {
          ...type.authorization,
          actions: type.authorization.actions.map((action) => ({
            ...action,
            label:
              locale.authorization?.actions?.[action.code]?.label ??
              action.label
          }))
        }
      : type.authorization
  };
}

/** Presentation only: raw query data, schema identities and business values stay intact. */
export function localizeNotificationType(
  type: NotificationType,
  locale: string,
  builtin?: NotificationTypeLocale
): NotificationType {
  if (type.type_code === 'system' && type.installation_id === null) {
    return builtin ? project(type, builtin) : type;
  }
  let display = type;
  const fallback = type.i18n?.en_US;
  if (fallback) display = project(display, fallback);
  const resolved = resolveAppLocale(locale);
  const requested = type.i18n?.[resolved];
  if (resolved !== 'en_US' && requested) display = project(display, requested);
  return display;
}
