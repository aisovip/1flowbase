import {
  useInfiniteQuery,
  useQuery,
  useQueryClient
} from '@tanstack/react-query';
import { useMemo, useState } from 'react';
import {
  applicationRunsQueryKey,
  fetchApplicationRuns,
  fetchApplicationLogConversations,
  type FetchApplicationRunsInput
} from '../../../api/runtime';

export function useApplicationRunsQuery(
  scope: string | string[],
  input: FetchApplicationRunsInput,
  mobile: boolean | undefined,
  canonicalConversations = false
) {
  const client = useQueryClient();
  const fetchPage = canonicalConversations
    ? fetchApplicationLogConversations
    : fetchApplicationRuns;
  const queryKey = (value: FetchApplicationRunsInput) =>
    canonicalConversations
      ? [...applicationRunsQueryKey(scope, value), 'canonical-conversations']
      : applicationRunsQueryKey(scope, value);
  const [refreshVersion, setRefreshVersion] = useState(0);
  const firstInput = { ...input, page: 1 };
  const mobileKey = [...queryKey(firstInput), 'mobile-list'];
  const desktop = useQuery({
    queryKey: queryKey(input),
    queryFn: () => fetchPage(scope, input),
    enabled: mobile === false
  });
  const infinite = useInfiniteQuery({
    queryKey: mobileKey,
    initialPageParam: 1,
    queryFn: ({ pageParam }) => fetchPage(scope, { ...input, page: pageParam }),
    getNextPageParam: (last, pages) =>
      pages.reduce((count, page) => count + page.items.length, 0) <
        last.total && last.items.length > 0
        ? pages.length + 1
        : undefined,
    enabled: mobile === true
  });
  const mobileItems = useMemo(() => {
    const items = infinite.data?.pages.flatMap((page) => page.items) ?? [];
    // Canonical pages already contain server-owned conversation rows.
    return canonicalConversations
      ? items
      : [...new Map(items.map((item) => [item.id, item])).values()];
  }, [canonicalConversations, infinite.data]);
  const query = mobile ? infinite : desktop;
  return {
    ...query,
    data: mobile
      ? infinite.data && {
          items: mobileItems,
          total: infinite.data.pages[0].total
        }
      : desktop.data,
    // A failed next page must preserve the records already displayed.
    isError: query.isError && !query.data,
    mobileList: mobile
      ? {
          resetKey: `${JSON.stringify(mobileKey)}:${refreshVersion}`,
          hasMore: infinite.hasNextPage,
          loading: infinite.isFetching,
          failed: infinite.isFetchNextPageError,
          onLoadMore: () => infinite.fetchNextPage({ cancelRefetch: false })
        }
      : undefined,
    async refresh() {
      const refreshed = await fetchPage(scope, {
        ...(mobile ? firstInput : input),
        cacheMode: 'refresh'
      });
      if (mobile) {
        await client.cancelQueries({ queryKey: mobileKey, exact: true });
        setRefreshVersion((version) => version + 1);
        client.setQueryData(mobileKey, { pages: [refreshed], pageParams: [1] });
      } else {
        client.setQueryData(queryKey(input), refreshed);
      }
    }
  };
}
