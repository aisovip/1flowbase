import { queryOptions } from '@tanstack/react-query';
import {
  getNotificationUnreadCount,
  listNotificationInstances,
  listNotificationMessages,
  listNotificationSendLogs,
  getNotificationSendLog,
  listNotificationTypes
} from '@1flowbase/api-client';
import type {
  NotificationInstanceListInput,
  NotificationLogListInput
} from '@1flowbase/api-client';
export {
  createNotificationInstance,
  updateNotificationInstance,
  deleteNotificationInstance,
  markNotificationRead,
  markAllNotificationsRead,
  subscribeNotificationChanges
} from '@1flowbase/api-client';
export const notificationQueryKey = (scope: string) =>
  ['notifications', scope] as const;
export const notificationInstancesOptions = (
  scope: string,
  input: NotificationInstanceListInput = {}
) =>
  queryOptions({
    queryKey: [...notificationQueryKey(scope), 'instances', input],
    queryFn: ({ signal }) => listNotificationInstances(input, signal)
  });
export const notificationTypesOptions = (scope: string) =>
  queryOptions({
    queryKey: [...notificationQueryKey(scope), 'types'],
    queryFn: ({ signal }) => listNotificationTypes(signal)
  });
export const notificationUnreadOptions = (scope: string) =>
  queryOptions({
    queryKey: [...notificationQueryKey(scope), 'unread-count'],
    queryFn: ({ signal }) => getNotificationUnreadCount(signal)
  });
export const notificationMessagesOptions = (
  scope: string,
  cursor?: string,
  unread_only = false
) =>
  queryOptions({
    queryKey: [
      ...notificationQueryKey(scope),
      'messages',
      cursor ?? '',
      unread_only
    ],
    queryFn: ({ signal }) =>
      listNotificationMessages({ cursor, unread_only }, signal)
  });

export const notificationLogsOptions = (
  scope: string,
  input: NotificationLogListInput
) =>
  queryOptions({
    queryKey: [...notificationQueryKey(scope), 'logs', input],
    queryFn: ({ signal }) => listNotificationSendLogs(input, signal)
  });
export const notificationLogOptions = (scope: string, id: string) =>
  queryOptions({
    queryKey: [...notificationQueryKey(scope), 'log', id],
    queryFn: ({ signal }) => getNotificationSendLog(id, signal)
  });
