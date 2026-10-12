import { useMemo, useState } from 'react';
import {
  useInfiniteQuery,
  useQuery,
  useQueryClient
} from '@tanstack/react-query';
import { Alert, Button, Empty, Input, Select, Spin } from 'antd';
import type { ConversationTrajectoryQuery } from '@1flowbase/api-client';
import type { ConversationLogTraceLoader } from '../../conversation-log-trace-model';
import { i18nText } from '../../../../../../shared/i18n/text';
import { formatDateTime } from '../../../../../../shared/i18n/format';
import { integrityLabel } from '../trajectory-presentation';
import { ClientTrajectoryDetail } from './ClientTrajectoryDetail';
import { categoryLabel, clientLane, sectionLabel } from './presentation';
import './client-trajectory.css';
const categories = [
  'request',
  'system',
  'user',
  'assistant',
  'reasoning',
  'tool_definition',
  'tool_call',
  'tool_result',
  'usage',
  'error'
];
const text = (key: string) =>
  (
    ({
      missing_identity: i18nText(
        'agentFlow',
        'conversation_trajectory.missing_identity'
      ),
      provenance: i18nText('agentFlow', 'conversation_trajectory.provenance'),
      all_sources: i18nText('agentFlow', 'conversation_trajectory.all_sources'),
      main: i18nText('agentFlow', 'conversation_trajectory.main'),
      subagent: i18nText('agentFlow', 'conversation_trajectory.subagent'),
      from: i18nText('agentFlow', 'conversation_trajectory.from'),
      to: i18nText('agentFlow', 'conversation_trajectory.to'),
      search_sections: i18nText(
        'agentFlow',
        'conversation_trajectory.search_sections'
      ),
      previous: i18nText('agentFlow', 'conversation_trajectory.previous'),
      next: i18nText('agentFlow', 'conversation_trajectory.next'),
      loaded: i18nText('agentFlow', 'conversation_trajectory.loaded'),
      search: i18nText('agentFlow', 'conversation_trajectory.search'),
      invalid_time: i18nText(
        'agentFlow',
        'conversation_trajectory.invalid_time'
      ),
      related_unloaded: i18nText(
        'agentFlow',
        'conversation_trajectory.related_unloaded'
      )
    }) as Record<string, string>
  )[key];
/** Canonical identity comes from the server; a record is only an initial focus. */
export function ConversationTrajectoryWorkspace({
  loader,
  recordId,
  logConversationId,
  active = true
}: {
  loader: ConversationLogTraceLoader;
  recordId: string;
  logConversationId?: string | null;
  active?: boolean;
}) {
  const overview = useQuery({
    queryKey: ['conversation-trajectory-identity', recordId],
    enabled:
      active &&
      !logConversationId &&
      Boolean(loader.loadRecordOverviewForTrajectory),
    queryFn: () => loader.loadRecordOverviewForTrajectory!(recordId),
    refetchOnWindowFocus: false
  });
  const canonical = logConversationId ?? overview.data?.log_conversation_id;
  if (canonical)
    return (
      <CanonicalTimeline
        key={`${canonical}:${recordId}`}
        loader={loader}
        logConversationId={canonical}
        focusRecordId={recordId}
        active={active}
      />
    );
  if (overview.isLoading) return <Spin />;
  if (overview.isError)
    return (
      <Alert
        type="error"
        title={i18nText('agentFlow', 'auto.loading_failed')}
        action={
          <Button onClick={() => void overview.refetch()}>
            {i18nText('agentFlow', 'auto.retry')}
          </Button>
        }
      />
    );
  return <Alert type="warning" title={text('missing_identity')} />;
}
function timestamp(value: string): string | undefined {
  if (!value) return undefined;
  const date = new Date(value);
  return Number.isFinite(date.getTime()) ? date.toISOString() : undefined;
}
function CanonicalTimeline({
  loader,
  logConversationId,
  focusRecordId,
  active
}: {
  loader: ConversationLogTraceLoader;
  logConversationId: string;
  focusRecordId: string;
  active: boolean;
}) {
  const queryClient = useQueryClient();
  const [category, setCategory] = useState<string>();
  const [provenance, setProvenance] = useState('all');
  const [from, setFrom] = useState('');
  const [to, setTo] = useState('');
  const [keyword, setKeyword] = useState('');
  const [searchSections, setSearchSections] = useState([
    'overview',
    'parameters',
    'result'
  ]);
  const [selected, setSelected] = useState<string | null>(null);
  const [generation, setGeneration] = useState(0);
  const [relatedTarget, setRelatedTarget] = useState<string | null>(null);
  const fromTimestamp = timestamp(from);
  const toTimestamp = timestamp(to);
  const invalidTime = Boolean((from && !fromTimestamp) || (to && !toTimestamp));
  const query: ConversationTrajectoryQuery = {
    filter: {
      ...(category ? { category: { $eq: category } } : {}),
      ...(provenance !== 'all'
        ? { is_subagent: { $eq: provenance === 'subagent' } }
        : {}),
      ...(fromTimestamp || toTimestamp
        ? {
            created_at: {
              ...(fromTimestamp ? { $gte: fromTimestamp } : {}),
              ...(toTimestamp ? { $lte: toTimestamp } : {})
            }
          }
        : {})
    },
    keyword: keyword || undefined,
    search_sections: searchSections,
    focus_record_id: focusRecordId,
    limit: 50
  };
  const pages = useInfiniteQuery({
    queryKey: [
      'conversation-client-trajectory',
      logConversationId,
      query,
      generation
    ],
    enabled:
      active &&
      !invalidTime &&
      Boolean(loader.queryConversationClientTrajectory),
    initialPageParam: undefined as string | undefined,
    queryFn: ({ pageParam }) =>
      loader.queryConversationClientTrajectory!(logConversationId, {
        ...query,
        cursor: pageParam
      }),
    getPreviousPageParam: (page) => page.previous_cursor ?? undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
    staleTime: Infinity,
    refetchOnWindowFocus: false,
    refetchOnMount: false
  });
  // Preserve server full-precision order, including duplicate timestamps.
  const items = pages.data?.pages.flatMap((page) => page.items) ?? [];
  const selectedStep = items.find((step) => step.id === selected);
  const detailLoader = useMemo<ConversationLogTraceLoader>(
    () => ({
      loadClientTrajectorySection: (
        canonical,
        stepId,
        section,
        _node,
        cursor
      ) =>
        loader.loadConversationClientTrajectorySection!(
          canonical,
          stepId,
          section,
          cursor
        )
    }),
    [loader]
  );
  return (
    <div className="provider-trajectory client-trajectory conversation-trajectory">
      <div className="provider-trajectory__toolbar">
        <div className="provider-trajectory__controls">
          <Select
            aria-label={text('provenance')}
            value={provenance}
            onChange={setProvenance}
            options={[
              { value: 'all', label: text('all_sources') },
              { value: 'main', label: text('main') },
              { value: 'subagent', label: text('subagent') }
            ]}
          />
          <Select
            aria-label={i18nText('agentFlow', 'client_trajectory.categories')}
            value={category ?? ''}
            onChange={(value) => setCategory(value || undefined)}
            options={[
              {
                value: '',
                label: i18nText('agentFlow', 'client_trajectory.all')
              },
              ...categories.map((value) => ({
                value,
                label: categoryLabel(value)
              }))
            ]}
          />
          <label className="conversation-trajectory__time">
            {text('from')}
            <input
              type="datetime-local"
              step="1"
              value={from}
              onChange={(event) => setFrom(event.target.value)}
            />
          </label>
          <label className="conversation-trajectory__time">
            {text('to')}
            <input
              type="datetime-local"
              step="1"
              value={to}
              onChange={(event) => setTo(event.target.value)}
            />
          </label>
          <Button
            onClick={() => {
              queryClient.removeQueries({
                queryKey: [
                  'conversation-client-trajectory',
                  logConversationId,
                  query,
                  generation
                ],
                exact: true
              });
              setSelected(null);
              setRelatedTarget(null);
              setGeneration((value) => value + 1);
            }}
            loading={pages.isFetching}
          >
            {i18nText('agentFlow', 'client_trajectory.refresh')}
          </Button>
        </div>
        <div className="conversation-trajectory__search">
          <Input.Search
            aria-label={text('search')}
            allowClear
            onSearch={(value) => setKeyword(value.trim())}
          />
          <Select
            mode="multiple"
            aria-label={text('search_sections')}
            value={searchSections}
            onChange={(values) =>
              setSearchSections(values.length ? values : ['result'])
            }
            options={['overview', 'parameters', 'result'].map((value) => ({
              value,
              label: sectionLabel(value)
            }))}
          />
        </div>
      </div>
      {invalidTime ? (
        <Alert type="warning" title={text('invalid_time')} />
      ) : null}
      <div className="provider-trajectory__split">
        <div className="provider-trajectory__ledger">
          {pages.hasPreviousPage ? (
            <Button
              loading={pages.isFetchingPreviousPage}
              disabled={pages.isFetching}
              onClick={() => void pages.fetchPreviousPage()}
            >
              {text('previous')}
            </Button>
          ) : null}
          {pages.isLoading ? (
            <div role="status">
              <Spin />
              {i18nText('agentFlow', 'trajectory.loading_first_page')}
            </div>
          ) : null}
          {pages.isError ? (
            <Alert
              type="error"
              title={i18nText('agentFlow', 'auto.loading_failed')}
              action={
                <Button
                  onClick={() =>
                    void (pages.isFetchPreviousPageError
                      ? pages.fetchPreviousPage()
                      : pages.isFetchNextPageError
                        ? pages.fetchNextPage()
                        : pages.refetch())
                  }
                >
                  {i18nText('agentFlow', 'auto.retry')}
                </Button>
              }
            />
          ) : null}
          {!pages.isPending && !pages.isError && !items.length ? (
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description={i18nText(
                'agentFlow',
                'client_trajectory.no_records'
              )}
            />
          ) : null}
          {items.map((step) => (
            <button
              key={`${step.record_id}:${step.id}`}
              type="button"
              className="provider-trajectory__row client-trajectory__row conversation-trajectory__row"
              data-lane={clientLane(step)}
              data-selected={selected === step.id || undefined}
              aria-pressed={selected === step.id}
              aria-label={`${categoryLabel(step.category)} · ${step.name}`}
              onClick={() => {
                setSelected(step.id);
                setRelatedTarget(null);
              }}
            >
              <span
                className="provider-trajectory__marker"
                aria-hidden="true"
              />
              <span className="provider-trajectory__kind">
                {categoryLabel(step.category)}
              </span>
              <span className="client-trajectory__summary">
                <strong>
                  {step.namespace
                    ? `${step.namespace}.${step.name}`
                    : step.name}
                </strong>
                <span className="provider-trajectory__preview">
                  {step.parameters_preview ||
                    step.result_preview ||
                    step.preview ||
                    '—'}
                </span>
                <small
                  className="conversation-trajectory__source"
                  title={`${step.source_client ?? step.source_kind} · ${step.source_session_id ?? '—'} · ${step.source_task_id ?? '—'} · ${step.record_id}`}
                >
                  {step.is_subagent ? text('subagent') : text('main')} ·{' '}
                  {step.source_client ?? step.source_kind} ·{' '}
                  {step.source_task_id ??
                    step.source_session_id ??
                    step.source_id ??
                    step.record_id}
                </small>
                {pages.data?.pages
                  .flatMap((page) => page.matches)
                  .filter((match) => match.step_id === step.id)
                  .map((match) => (
                    <small
                      key={`${match.section}:${match.sequence}`}
                      className="conversation-trajectory__match"
                    >
                      {sectionLabel(match.section)} · {match.snippet}
                    </small>
                  ))}
              </span>
              <time
                className="provider-trajectory__row-time"
                dateTime={step.created_at}
                title={step.created_at}
              >
                {formatDateTime(step.created_at)}
              </time>
            </button>
          ))}
          {pages.hasNextPage ? (
            <Button
              loading={pages.isFetchingNextPage}
              disabled={pages.isFetching}
              onClick={() => void pages.fetchNextPage()}
            >
              {text('next')}
            </Button>
          ) : null}
        </div>
        {selectedStep ? (
          <aside
            className="provider-trajectory__inspector"
            aria-label={i18nText('agentFlow', 'trajectory.inspector')}
          >
            <div className="provider-trajectory__inspector-header">
              <strong>{selectedStep.name}</strong>
              <Button onClick={() => setSelected(null)}>
                {i18nText('agentFlow', 'client_trajectory.close')}
              </Button>
            </div>
            {relatedTarget ? (
              <Alert
                type="info"
                title={text('related_unloaded')}
                action={
                  items.some((step) => step.id === relatedTarget) ? (
                    <Button
                      onClick={() => {
                        setSelected(relatedTarget);
                        setRelatedTarget(null);
                      }}
                    >
                      {i18nText('agentFlow', 'client_trajectory.related')}
                    </Button>
                  ) : (
                    <div>
                      {pages.hasPreviousPage ? (
                        <Button
                          disabled={pages.isFetching}
                          onClick={() => void pages.fetchPreviousPage()}
                        >
                          {text('previous')}
                        </Button>
                      ) : null}
                      {pages.hasNextPage ? (
                        <Button
                          disabled={pages.isFetching}
                          onClick={() => void pages.fetchNextPage()}
                        >
                          {text('next')}
                        </Button>
                      ) : null}
                    </div>
                  )
                }
              />
            ) : null}
            <ClientTrajectoryDetail
              key={`${generation}:${selectedStep.id}`}
              step={selectedStep}
              runId={logConversationId}
              loader={detailLoader}
              onRelated={(id) => {
                if (items.some((step) => step.id === id)) {
                  setSelected(id);
                  setRelatedTarget(null);
                } else setRelatedTarget(id);
              }}
            />
          </aside>
        ) : null}
      </div>
      <footer className="provider-trajectory__footer">
        <span role="status">
          {pages.isFetching
            ? i18nText('agentFlow', 'trajectory.loading_pages')
            : text('loaded')}
        </span>
        <span>
          {i18nText('agentFlow', 'trajectory.loaded_steps', {
            count: items.length
          })}
        </span>
        <span
          className="provider-trajectory__integrity"
          data-status={pages.data?.pages[0]?.integrity}
        >
          {integrityLabel(
            pages.data?.pages[0]?.integrity as Parameters<
              typeof integrityLabel
            >[0]
          )}
        </span>
      </footer>
    </div>
  );
}
