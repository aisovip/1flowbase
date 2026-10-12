import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import {
  useInfiniteQuery,
  useQuery,
  useQueryClient
} from '@tanstack/react-query';
import { Alert, Button, Empty, Input, Popover, Select, Spin } from 'antd';
import type {
  ConversationTrajectoryQuery,
  ConversationClientTrajectoryStep
} from '@1flowbase/api-client';
import type { ConversationLogTraceLoader } from '../../conversation-log-trace-model';
import { i18nText } from '../../../../../../shared/i18n/text';
import { formatDateTime } from '../../../../../../shared/i18n/format';
import CloseOutlined from '@ant-design/icons/es/icons/CloseOutlined';
import FilterOutlined from '@ant-design/icons/es/icons/FilterOutlined';
import SearchOutlined from '@ant-design/icons/es/icons/SearchOutlined';
import {
  TrajectoryTimeline,
  type TrajectoryRange
} from '../TrajectoryTimeline';
import { useProgressiveTrajectory } from '../use-progressive-trajectory';
import { integrityLabel } from '../trajectory-presentation';
import { ClientTrajectoryDetail } from './ClientTrajectoryDetail';
import { categoryLabel, clientLane, sectionLabel } from './presentation';
import './client-trajectory.css';
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
      loaded: i18nText('agentFlow', 'conversation_trajectory.loaded'),
      search: i18nText('agentFlow', 'conversation_trajectory.search'),
      invalid_time: i18nText(
        'agentFlow',
        'conversation_trajectory.invalid_time'
      ),
      filters: i18nText('agentFlow', 'conversation_trajectory.filters'),
      loaded_categories: i18nText(
        'agentFlow',
        'conversation_trajectory.loaded_categories'
      ),
      loaded_positions: i18nText(
        'agentFlow',
        'conversation_trajectory.loaded_positions'
      ),
      subagent_marker: i18nText(
        'agentFlow',
        'conversation_trajectory.subagent_marker'
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
  const [timeScale, setTimeScale] = useState(true);
  const [timeRange, setTimeRange] = useState<TrajectoryRange>(null);
  const [brushItems, setBrushItems] = useState<
    ConversationClientTrajectoryStep[] | null
  >(null);
  const preserveRangeOnSelect = useRef(false);
  const pointerStart = useRef<number | null>(null);
  const pointerMoved = useRef(false);
  const committedSelection = useRef<{
    range: TrajectoryRange;
    items: ConversationClientTrajectoryStep[] | null;
  }>({ range: null, items: null });
  const pendingSelection = useRef<{
    range: TrajectoryRange;
    items: ConversationClientTrajectoryStep[] | null;
  } | null>(null);
  const pendingAxisFilter = useRef<Record<string, string> | null>(null);
  const [axisFilter, setAxisFilter] = useState<Record<string, string> | null>(
    null
  );
  const [groupCategories, setGroupCategories] = useState(false);
  const [seenCategories, setSeenCategories] = useState<string[]>([]);
  const [detailWidth, setDetailWidth] = useState<number | null>(null);
  const split = useRef<HTMLDivElement>(null);
  const drag = useRef<{ x: number; width: number } | null>(null);
  const rows = useRef(new Map<string, HTMLButtonElement>());
  const ledger = useRef<HTMLDivElement>(null);
  const scrollAnchor = useRef<{
    element: HTMLButtonElement;
    top: number;
  } | null>(null);
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
  const [relatedStep, setRelatedStep] =
    useState<ConversationClientTrajectoryStep | null>(null);
  const fromTimestamp = timestamp(from);
  const toTimestamp = timestamp(to);
  const invalidTime = Boolean((from && !fromTimestamp) || (to && !toTimestamp));
  const query: ConversationTrajectoryQuery = {
    filter: {
      ...(category ? { category: { $eq: category } } : {}),
      ...(provenance !== 'all'
        ? { is_subagent: { $eq: provenance === 'subagent' } }
        : {}),
      ...(axisFilter
        ? { created_at: axisFilter }
        : fromTimestamp || toTimestamp
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
  useProgressiveTrajectory(active && !invalidTime, pages);
  // Preserve server full-precision order, including duplicate timestamps.
  const items = useMemo(
    () => pages.data?.pages.flatMap((page) => page.items) ?? [],
    [pages.data]
  );
  const matchesByStep = useMemo(() => {
    const matches = new Map<
      string,
      NonNullable<typeof pages.data>['pages'][number]['matches']
    >();
    for (const page of pages.data?.pages ?? [])
      for (const match of page.matches) {
        const entries = matches.get(match.step_id) ?? [];
        entries.push(match);
        matches.set(match.step_id, entries);
      }
    return matches;
  }, [pages.data]);
  useLayoutEffect(() => {
    const anchor = scrollAnchor.current;
    // Keep the focused round/user's viewport in place as older pages prepend.
    // If the browser already anchored it, the measured adjustment is zero.
    const first = items[0] && rows.current.get(items[0].id);
    if (
      ledger.current &&
      anchor?.element.isConnected &&
      first !== anchor.element
    )
      ledger.current.scrollTop +=
        anchor.element.getBoundingClientRect().top -
        ledger.current.getBoundingClientRect().top -
        anchor.top;
    scrollAnchor.current = first
      ? {
          element: first,
          top:
            first.getBoundingClientRect().top -
            (ledger.current?.getBoundingClientRect().top ?? 0)
        }
      : null;
  }, [items]);
  const related = useQuery({
    queryKey: [
      'conversation-trajectory-related',
      logConversationId,
      relatedTarget,
      generation
    ],
    enabled:
      active &&
      Boolean(relatedTarget && loader.queryConversationClientTrajectory),
    queryFn: () =>
      loader.queryConversationClientTrajectory!(logConversationId, {
        filter: { id: { $eq: relatedTarget } },
        limit: 1
      }),
    retry: false,
    refetchOnWindowFocus: false
  });
  useEffect(() => {
    if (relatedTarget && related.data?.items[0])
      setRelatedStep(related.data.items[0]);
  }, [relatedTarget, related.data]);
  const selectedStep =
    relatedStep ?? items.find((step) => step.id === selected);
  const timelineItems = brushItems ?? items;
  const coordinates = new Map(
    timelineItems.map((step, index) => [
      step.id,
      timeScale ? Date.parse(step.created_at) : index
    ])
  );
  useEffect(() => {
    setSeenCategories((current) => [
      ...new Set([...current, ...items.map((step) => step.category)])
    ]);
  }, [pages.data]);
  function select(id: string) {
    preserveRangeOnSelect.current = false;
    if (items.some((step) => step.id === id)) {
      setSelected(id);
      setRelatedTarget(null);
      setRelatedStep(null);
    } else {
      // An axis point can belong to the retained display domain, outside current filters.
      if (!selectedStep)
        setRelatedStep(timelineItems.find((step) => step.id === id) ?? null);
      setRelatedTarget(id);
    }
    requestAnimationFrame(() =>
      rows.current.get(id)?.scrollIntoView?.({ block: 'nearest' })
    );
  }
  function changeRange(range: TrajectoryRange) {
    if (!range && preserveRangeOnSelect.current) return;
    setTimeRange(range);
    if (!range) {
      pendingAxisFilter.current = null;
      pendingSelection.current = null;
      committedSelection.current = { range: null, items: null };
      setAxisFilter(null);
      setBrushItems(null);
      return;
    }
    setBrushItems(timelineItems);
    pendingSelection.current = { range, items: timelineItems };
    if (timeScale) {
      // Inclusive display milliseconds include persisted sub-millisecond events.
      pendingAxisFilter.current = {
        $gte: new Date(Math.floor(range[0])).toISOString(),
        $lt: new Date(Math.floor(range[1]) + 1).toISOString()
      };
    } else {
      const first =
        timelineItems[
          Math.max(0, Math.min(timelineItems.length - 1, Math.ceil(range[0])))
        ];
      const last =
        timelineItems[
          Math.max(0, Math.min(timelineItems.length - 1, Math.floor(range[1])))
        ];
      if (first && last)
        pendingAxisFilter.current =
          Math.ceil(range[0]) > Math.floor(range[1])
            ? { $gt: first.created_at, $lte: first.created_at }
            : { $gte: first.created_at, $lte: last.created_at };
    }
  }
  function commitRange() {
    if (pendingAxisFilter.current) {
      setAxisFilter(pendingAxisFilter.current);
      if (pendingSelection.current)
        committedSelection.current = pendingSelection.current;
      pendingAxisFilter.current = null;
      pendingSelection.current = null;
    }
  }
  function cancelRange() {
    pendingAxisFilter.current = null;
    pendingSelection.current = null;
    pointerStart.current = null;
    pointerMoved.current = false;
    preserveRangeOnSelect.current = false;
    setTimeRange(committedSelection.current.range);
    setBrushItems(committedSelection.current.items);
  }
  function resize(width: number) {
    setDetailWidth(
      Math.max(
        320,
        Math.min(720, (split.current?.clientWidth ?? 1000) - 280, width)
      )
    );
  }
  function refresh() {
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
    setRelatedStep(null);
    setBrushItems(null);
    setGeneration((value) => value + 1);
  }
  function renderStep(step: ConversationClientTrajectoryStep) {
    const matches = matchesByStep.get(step.id) ?? [];
    const identity = `${step.is_subagent ? text('subagent') : text('main')} · ${step.source_client ?? step.source_kind} · ${step.source_session_id ?? '—'} · ${step.source_task_id ?? '—'} · ${step.source_id ?? '—'} · ${step.record_id}`;
    return (
      <button
        key={`${step.record_id}:${step.id}`}
        type="button"
        ref={(element) => {
          if (element) rows.current.set(step.id, element);
          else rows.current.delete(step.id);
        }}
        className="provider-trajectory__row client-trajectory__row conversation-trajectory__row"
        data-lane={clientLane(step)}
        data-selected={selectedStep?.id === step.id || undefined}
        aria-pressed={selectedStep?.id === step.id}
        aria-label={`${categoryLabel(step.category)} · ${step.name}`}
        onClick={() => select(step.id)}
      >
        <span className="provider-trajectory__marker" aria-hidden="true" />
        <span className="provider-trajectory__kind">
          {categoryLabel(step.category)}
        </span>
        <span className="client-trajectory__summary">
          <strong>
            {step.namespace ? `${step.namespace}.${step.name}` : step.name}
          </strong>
          <span
            className="provider-trajectory__preview"
            title={
              matches
                .map(
                  (match) => `${sectionLabel(match.section)} · ${match.snippet}`
                )
                .join('\n') || step.preview
            }
          >
            {step.parameters_preview ||
              step.result_preview ||
              step.preview ||
              '—'}
          </span>
          {step.is_subagent ? (
            <small className="conversation-trajectory__source" title={identity}>
              {text('subagent_marker')}
            </small>
          ) : null}
        </span>
        <span
          className="provider-trajectory__row-time"
          title={`${formatDateTime(step.created_at)} · ${step.created_at} · ${identity}`}
        >
          #{step.sequence}
        </span>
      </button>
    );
  }
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
          <span className="client-trajectory__protocol">
            {[...new Set(items.map((step) => step.protocol))]
              .map((protocol) =>
                protocol === 'chat_completions'
                  ? 'Chat Completions'
                  : protocol === 'responses'
                    ? 'Responses'
                    : protocol
              )
              .join(' · ')}
          </span>
          <Button
            size="small"
            type="text"
            aria-pressed={timeScale}
            onClick={() => {
              setTimeScale(!timeScale);
              changeRange(null);
            }}
          >
            {i18nText(
              'agentFlow',
              timeScale ? 'trajectory.time_axis' : 'trajectory.sequence_axis'
            )}
          </Button>
          <Button
            size="small"
            type="text"
            aria-pressed={groupCategories}
            onClick={() => setGroupCategories(!groupCategories)}
          >
            {i18nText('agentFlow', 'client_trajectory.group_categories')}
          </Button>
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
          <Button
            size="small"
            type="text"
            loading={pages.isFetching}
            onClick={refresh}
          >
            {i18nText('agentFlow', 'client_trajectory.refresh')}
          </Button>
          <Popover
            trigger="click"
            placement="bottomRight"
            content={
              <div className="conversation-trajectory__filters">
                <Select
                  size="small"
                  aria-label={i18nText(
                    'agentFlow',
                    'client_trajectory.categories'
                  )}
                  value={category ?? ''}
                  onChange={(value) => setCategory(value || undefined)}
                  options={[
                    {
                      value: '',
                      label: i18nText('agentFlow', 'client_trajectory.all')
                    },
                    ...[
                      ...new Set([
                        'request',
                        'system',
                        'user',
                        'assistant',
                        'reasoning',
                        'tool_definition',
                        'tool_call',
                        'tool_result',
                        'usage',
                        'error',
                        'context',
                        'task_start',
                        'task_end',
                        ...seenCategories
                      ])
                    ].map((value) => ({ value, label: categoryLabel(value) }))
                  ]}
                />
                <label className="conversation-trajectory__time">
                  {text('from')}
                  <input
                    type="datetime-local"
                    step="1"
                    value={from}
                    onChange={(event) => {
                      setFrom(event.target.value);
                      changeRange(null);
                    }}
                  />
                </label>
                <label className="conversation-trajectory__time">
                  {text('to')}
                  <input
                    type="datetime-local"
                    step="1"
                    value={to}
                    onChange={(event) => {
                      setTo(event.target.value);
                      changeRange(null);
                    }}
                  />
                </label>
                <Select
                  size="small"
                  mode="multiple"
                  aria-label={text('search_sections')}
                  value={searchSections}
                  onChange={(values) =>
                    setSearchSections(values.length ? values : ['result'])
                  }
                  options={['overview', 'parameters', 'result'].map(
                    (value) => ({ value, label: sectionLabel(value) })
                  )}
                />
              </div>
            }
          >
            <Button
              size="small"
              type="text"
              icon={<FilterOutlined />}
              aria-label={text('filters')}
            />
          </Popover>
        </div>
        <Input
          className="provider-trajectory__search"
          size="small"
          prefix={<SearchOutlined />}
          aria-label={text('search')}
          allowClear
          onPressEnter={(event) => setKeyword(event.currentTarget.value.trim())}
          onChange={(event) => {
            if (!event.target.value) setKeyword('');
          }}
          placeholder={text('search')}
        />
      </div>
      <div
        className="conversation-trajectory__axis"
        onPointerDownCapture={(event) => {
          pointerStart.current = event.clientX;
          pointerMoved.current = false;
        }}
        onPointerMoveCapture={(event) => {
          if (
            pointerStart.current !== null &&
            Math.abs(event.clientX - pointerStart.current) >= 3
          )
            pointerMoved.current = true;
        }}
        onPointerUpCapture={() => {
          preserveRangeOnSelect.current = !pointerMoved.current;
          pointerStart.current = null;
        }}
        onClickCapture={(event) => {
          preserveRangeOnSelect.current = Boolean(
            (event.target as HTMLElement).closest('.provider-trajectory__block')
          );
        }}
        onPointerUp={commitRange}
        onPointerCancel={cancelRange}
        onKeyDown={(event) => {
          if (
            (event.target as HTMLElement).dataset.brush &&
            ['ArrowLeft', 'ArrowRight', 'Home', 'End', 'Escape'].includes(
              event.key
            )
          )
            commitRange();
        }}
      >
        <TrajectoryTimeline
          points={timelineItems.map((step) => ({
            id: step.id,
            value: coordinates.get(step.id)!,
            lane: clientLane(step),
            label: `${categoryLabel(step.category)} #${step.sequence}`
          }))}
          timeScale={timeScale}
          range={timeRange}
          onChange={changeRange}
          selected={selectedStep?.id ?? selected}
          onSelect={select}
        />
      </div>
      {!timeScale ? (
        <div className="client-trajectory__note">
          {text('loaded_positions')}
        </div>
      ) : null}
      {invalidTime ? (
        <Alert type="warning" title={text('invalid_time')} />
      ) : null}
      <div className="provider-trajectory__split" ref={split}>
        <div
          className="provider-trajectory__ledger"
          ref={ledger}
          onScroll={() => {
            const anchor = scrollAnchor.current;
            if (anchor && ledger.current)
              anchor.top =
                anchor.element.getBoundingClientRect().top -
                ledger.current.getBoundingClientRect().top;
          }}
        >
          <nav
            className="client-trajectory__categories"
            aria-label={text('loaded_categories')}
            title={text('loaded_categories')}
          >
            <button
              type="button"
              aria-pressed={!category}
              onClick={() => setCategory(undefined)}
            >
              {i18nText('agentFlow', 'client_trajectory.all')}{' '}
              <span>{items.length}</span>
            </button>
            {seenCategories.map((key) => (
              <button
                type="button"
                key={key}
                aria-label={categoryLabel(key)}
                aria-pressed={category === key}
                onClick={() => setCategory(category === key ? undefined : key)}
              >
                {categoryLabel(key)}{' '}
                <span>
                  {items.filter((step) => step.category === key).length}
                </span>
              </button>
            ))}
            <span
              className="conversation-trajectory__provenance"
              role="group"
              aria-label={text('provenance')}
            >
              <button
                type="button"
                aria-pressed={provenance === 'all'}
                onClick={() => setProvenance('all')}
              >
                {text('all_sources')}
              </button>
              <button
                type="button"
                aria-pressed={provenance === 'main'}
                onClick={() =>
                  setProvenance(provenance === 'main' ? 'all' : 'main')
                }
              >
                {text('main')}
              </button>
              <button
                type="button"
                aria-pressed={provenance === 'subagent'}
                onClick={() =>
                  setProvenance(provenance === 'subagent' ? 'all' : 'subagent')
                }
              >
                {text('subagent')}
              </button>
            </span>
          </nav>
          {pages.isLoading ? (
            <div className="provider-trajectory__loading" role="status">
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
          {!groupCategories
            ? items.map(renderStep)
            : [...new Set(items.map((step) => step.category))].map((key) => (
                <section
                  key={key}
                  className="client-trajectory__category-group"
                >
                  <div className="client-trajectory__category-header">
                    <span>{categoryLabel(key)}</span>
                    <small>
                      {items.filter((step) => step.category === key).length}
                    </small>
                  </div>
                  {items
                    .filter((step) => step.category === key)
                    .map(renderStep)}
                </section>
              ))}
        </div>
        {selectedStep ? (
          <aside
            className="provider-trajectory__inspector"
            data-lane={clientLane(selectedStep)}
            aria-label={i18nText('agentFlow', 'trajectory.inspector')}
            style={detailWidth === null ? undefined : { width: detailWidth }}
          >
            <div
              className="provider-trajectory__resize"
              role="separator"
              tabIndex={0}
              aria-orientation="vertical"
              aria-label={i18nText('agentFlow', 'trajectory.inspector')}
              aria-valuemin={320}
              aria-valuemax={720}
              aria-valuenow={detailWidth ?? 440}
              onPointerDown={(event) => {
                drag.current = {
                  x: event.clientX,
                  width:
                    event.currentTarget.parentElement!.getBoundingClientRect()
                      .width
                };
                event.currentTarget.setPointerCapture(event.pointerId);
              }}
              onPointerMove={(event) => {
                if (drag.current)
                  resize(drag.current.width + drag.current.x - event.clientX);
              }}
              onPointerUp={() => {
                drag.current = null;
              }}
              onPointerCancel={() => {
                drag.current = null;
              }}
              onKeyDown={(event) => {
                if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
                  resize(
                    event.currentTarget.parentElement!.getBoundingClientRect()
                      .width + (event.key === 'ArrowLeft' ? 16 : -16)
                  );
                  event.preventDefault();
                }
              }}
            />
            <div className="provider-trajectory__inspector-header">
              <span className="provider-trajectory__kind">
                {categoryLabel(selectedStep.category)}
              </span>
              <strong className="provider-trajectory__preview">
                {selectedStep.namespace
                  ? `${selectedStep.namespace}.${selectedStep.name}`
                  : selectedStep.name}
              </strong>
              <Button
                size="small"
                type="text"
                icon={<CloseOutlined />}
                aria-label={i18nText('agentFlow', 'client_trajectory.close')}
                onClick={() => {
                  setSelected(null);
                  setRelatedTarget(null);
                  setRelatedStep(null);
                  rows.current.get(selectedStep.id)?.focus();
                }}
              />
            </div>
            {relatedTarget && related.isFetching ? (
              <div role="status">
                <Spin />
                {i18nText('agentFlow', 'trajectory.loading_first_page')}
              </div>
            ) : null}
            {relatedTarget && related.isError ? (
              <Alert
                type="error"
                title={i18nText('agentFlow', 'auto.loading_failed')}
                action={
                  <Button onClick={() => void related.refetch()}>
                    {i18nText('agentFlow', 'auto.retry')}
                  </Button>
                }
              />
            ) : null}
            {relatedTarget &&
            related.isSuccess &&
            !related.data.items.length ? (
              <Alert type="warning" title={text('related_unloaded')} />
            ) : null}
            <ClientTrajectoryDetail
              key={`${generation}:${selectedStep.id}`}
              step={selectedStep}
              runId={logConversationId}
              loader={detailLoader}
              onRelated={(id) => {
                if (items.some((step) => step.id === id)) select(id);
                else setRelatedTarget(id);
              }}
            />
          </aside>
        ) : null}
      </div>
      <footer className="provider-trajectory__footer">
        <span role="status">
          {!pages.isError &&
          (pages.isFetching || pages.hasPreviousPage || pages.hasNextPage)
            ? i18nText('agentFlow', 'trajectory.loading_pages')
            : text('loaded')}
        </span>
        <span>
          {i18nText('agentFlow', 'trajectory.loaded_steps', {
            count: items.length
          })}
        </span>
        <span>{i18nText('agentFlow', 'client_trajectory.client')}</span>
      </footer>
    </div>
  );
}
