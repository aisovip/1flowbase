import { useEffect } from 'react';

/** Load summary pages serially, yielding between pages; hidden views retain their cache. */
export function useProgressiveTrajectory(
  active: boolean,
  pages: {
    data?: { pages: unknown[] };
    hasNextPage: boolean;
    hasPreviousPage?: boolean;
    fetchPreviousPage?: () => Promise<unknown>;
    isFetching: boolean;
    isError: boolean;
    fetchNextPage: () => Promise<unknown>;
  }
) {
  const {
    hasNextPage,
    hasPreviousPage,
    isFetching,
    isError,
    fetchNextPage,
    fetchPreviousPage
  } = pages;
  // A focused page can have older and newer neighbors. Drain one direction at
  // a time so React Query never cancels an in-flight page with the other one.
  const fetchPage = hasPreviousPage
    ? fetchPreviousPage
    : hasNextPage
      ? fetchNextPage
      : undefined;
  // Immediate responses can batch fetching transitions; completed pages still advance.
  const completedPageCount = pages.data?.pages.length ?? 0;
  useEffect(() => {
    if (!active || !fetchPage || isFetching || isError) return;
    const timer = window.setTimeout(() => void fetchPage(), 100);
    return () => window.clearTimeout(timer);
  }, [active, completedPageCount, isFetching, isError, fetchPage]);
}
