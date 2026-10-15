import { useCallback, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import type {
  NotificationType,
  NotificationTypeLocale
} from '@1flowbase/api-client';
import { localizeNotificationType } from '../lib/type-i18n';

export function useNotificationTypeLocalization() {
  const { t, i18n } = useTranslation('notifications');
  const locale = i18n.resolvedLanguage ?? i18n.language;
  const builtin = useMemo<NotificationTypeLocale>(
    () => ({
      display_name: t('system_notification_type'),
      description: t('system_notification_description'),
      input_form: {
        fields: {
          user_ids: { label: t('system_notification_recipients') },
          title: { label: t('message_title') },
          content: { label: t('message_content') }
        }
      }
    }),
    [t]
  );
  return useCallback(
    (type: NotificationType) => localizeNotificationType(type, locale, builtin),
    [locale, builtin]
  );
}
