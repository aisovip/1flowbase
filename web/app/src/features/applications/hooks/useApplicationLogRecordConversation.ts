import { useEffect, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import type {
  ApplicationLogRecordConversationPage,
  ConsoleApplicationLogRecordOverview
} from '@1flowbase/api-client';
import { fetchApplicationLogRecordConversationMessages } from '../api/trajectory';

const PAGE_LIMIT = 5;
// Match the native catch-up scheduling batch, not a conversation capacity.
// Any remaining backlog resumes from the committed edge on the next summary refresh.
const FORWARD_BATCH_PAGE_LIMIT = 20;
const REFRESH_INTERVAL_MS = 1_000;

function mergeRecords(
  current: ConsoleApplicationLogRecordOverview[],
  incoming: ConsoleApplicationLogRecordOverview[]
) {
  const records = new Map(current.map((record) => [record.record_id, record]));
  for (const record of incoming) records.set(record.record_id, record);
  return [...records.values()].sort(
    (left, right) =>
      left.started_at.localeCompare(right.started_at) ||
      left.record_id.localeCompare(right.record_id)
  );
}

/** Retain displayed main turns while reading new facts forward from an observed edge. */
export function useApplicationLogRecordConversation(
  applicationId: string,
  recordId?: string | null
) {
  const [records, setRecords] = useState<ConsoleApplicationLogRecordOverview[]>(
    []
  );
  const [historyAnchor, setHistoryAnchor] =
    useState<ApplicationLogRecordConversationPage | null>(null);
  const [loadingHistory, setLoadingHistory] = useState(false);
  const [historyFailed, setHistoryFailed] = useState(false);
  const [forwardFailed, setForwardFailed] = useState(false);
  const newestEdge = useRef<string | null>(null);
  const targetNewestEdge = useRef<string | null>(null);
  const conversationGeneration = useRef(0);
  const forwardBusy = useRef(false);
  const historyBusy = useRef(false);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    conversationGeneration.current += 1;
    newestEdge.current = null;
    targetNewestEdge.current = null;
    forwardBusy.current = false;
    historyBusy.current = false;
    setRecords([]);
    setHistoryAnchor(null);
    setLoadingHistory(false);
    setHistoryFailed(false);
    setForwardFailed(false);
    return () => {
      mounted.current = false;
      conversationGeneration.current += 1;
    };
  }, [applicationId, recordId]);
  const latest = useQuery({
    queryKey: [
      'application-log-record-conversation',
      applicationId,
      recordId,
      'latest'
    ],
    enabled: Boolean(recordId),
    queryFn: () =>
      fetchApplicationLogRecordConversationMessages(applicationId, recordId!, {
        before: undefined,
        limit: PAGE_LIMIT
      }),
    refetchInterval: REFRESH_INTERVAL_MS,
    refetchOnWindowFocus: false,
    refetchOnMount: 'always'
  });

  useEffect(() => {
    const page = latest.data;
    if (!recordId || !page) return;
    setRecords((current) => mergeRecords(current, page.items));
    setHistoryAnchor((current) => current ?? page);
    // Summary refreshes notify the target watermark; they do not own or cancel
    // an in-flight forward request. Only conversation identity/unmount does.
    targetNewestEdge.current = page.page.newest_cursor;
    if (!newestEdge.current) {
      newestEdge.current = page.page.newest_cursor;
      return;
    }
    if (newestEdge.current === targetNewestEdge.current || forwardBusy.current)
      return;
    const generation = conversationGeneration.current;
    forwardBusy.current = true;
    setForwardFailed(false);
    void (async () => {
      try {
        let after = newestEdge.current;
        for (
          let count = 0;
          after && count < FORWARD_BATCH_PAGE_LIMIT;
          count += 1
        ) {
          const forward = await fetchApplicationLogRecordConversationMessages(
            applicationId,
            recordId,
            { after, limit: PAGE_LIMIT }
          );
          if (!mounted.current || generation !== conversationGeneration.current)
            return;
          setRecords((current) => mergeRecords(current, forward.items));
          // Commit only successfully received edges. A failed or interrupted request
          // retries from this position; the newest summary never skips a backlog.
          const next = forward.page.has_after
            ? forward.page.after_cursor
            : forward.page.newest_cursor;
          if (!next || next === after) break;
          newestEdge.current = next;
          after = next;
          if (!forward.page.has_after) break;
        }
      } catch {
        if (mounted.current && generation === conversationGeneration.current)
          setForwardFailed(true);
      } finally {
        if (generation === conversationGeneration.current)
          forwardBusy.current = false;
      }
    })();
  }, [applicationId, recordId, latest.data, latest.dataUpdatedAt]);

  const hasNextPage = Boolean(
    historyAnchor?.page.has_before && historyAnchor.page.before_cursor
  );
  async function fetchNextPage() {
    const before = historyAnchor?.page.before_cursor;
    if (!recordId || !hasNextPage || !before || historyBusy.current) return;
    const generation = conversationGeneration.current;
    historyBusy.current = true;
    setLoadingHistory(true);
    setHistoryFailed(false);
    try {
      const page = await fetchApplicationLogRecordConversationMessages(
        applicationId,
        recordId,
        { before, limit: PAGE_LIMIT }
      );
      if (!mounted.current || generation !== conversationGeneration.current)
        return;
      setRecords((current) => mergeRecords(current, page.items));
      setHistoryAnchor(page);
    } catch {
      if (mounted.current && generation === conversationGeneration.current)
        setHistoryFailed(true);
    } finally {
      if (generation === conversationGeneration.current) {
        historyBusy.current = false;
        if (mounted.current) setLoadingHistory(false);
      }
    }
  }
  return {
    records,
    isLoading: latest.isLoading,
    isSuccess: latest.isSuccess,
    isError: latest.isError || historyFailed || forwardFailed,
    hasNextPage,
    isFetchingNextPage: loadingHistory,
    fetchNextPage
  };
}
