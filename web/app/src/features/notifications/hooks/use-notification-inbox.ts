import { useEffect, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useAuthStore } from '../../../state/auth-store';
import { selectNavigationQueryScope } from '../../../state/navigation-query-scope';
import {
  markAllNotificationsRead,
  markNotificationRead,
  notificationMessagesOptions,
  notificationQueryKey,
  notificationUnreadOptions,
  subscribeNotificationChanges
} from '../api/queries';

export function useNotificationInbox(open: boolean) {
  const scope = useAuthStore(selectNavigationQueryScope);
  const authenticated = useAuthStore(
    (state) => state.sessionStatus === 'authenticated'
  );
  const csrfToken = useAuthStore((state) => state.csrfToken) ?? '';
  const client = useQueryClient();
  const [cursor, setCursor] = useState<string>();
  const [previousCursors, setPreviousCursors] = useState<
    Array<string | undefined>
  >([]);
  const [unread_only, setUnreadOnly] = useState(false);
  const [connected, setConnected] = useState(false);
  const key = notificationQueryKey(scope);
  const refresh = () => client.invalidateQueries({ queryKey: key });
  const unread = useQuery({
    ...notificationUnreadOptions(scope),
    enabled: authenticated
  });
  const messages = useQuery({
    ...notificationMessagesOptions(scope, cursor, unread_only),
    enabled: authenticated && open
  });
  const read = useMutation({
    mutationFn: (id: string) => markNotificationRead(id, csrfToken),
    onSuccess: refresh
  });
  const readAll = useMutation({
    mutationFn: () => markAllNotificationsRead(csrfToken),
    onSuccess: refresh
  });
  useEffect(() => {
    setCursor(undefined);
    setPreviousCursors([]);
    setConnected(false);
    if (!authenticated) return;
    return subscribeNotificationChanges(
      () =>
        void client.invalidateQueries({
          queryKey: notificationQueryKey(scope)
        }),
      setConnected
    );
  }, [authenticated, client, scope]);
  return {
    authenticated,
    unread,
    messages,
    read,
    readAll,
    connected,
    refresh,
    unread_only,
    setUnreadOnly: (value: boolean) => {
      setUnreadOnly(value);
      setCursor(undefined);
      setPreviousCursors([]);
    },
    hasPrevious: previousCursors.length > 0,
    previous: () => {
      setCursor(previousCursors.at(-1));
      setPreviousCursors((values) => values.slice(0, -1));
    },
    next: () => {
      if (messages.data?.next_cursor) {
        setPreviousCursors((values) => [...values, cursor]);
        setCursor(messages.data.next_cursor);
      }
    }
  };
}
