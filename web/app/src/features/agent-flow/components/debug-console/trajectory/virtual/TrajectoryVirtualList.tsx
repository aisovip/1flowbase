import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useRef,
  useState,
  type ForwardedRef,
  type ReactNode,
  type RefAttributes
} from 'react';
import { Listy } from 'antd';
import type { ListyRef } from 'antd/es/listy';
import './trajectory-virtual-list.css';

export type TrajectoryVirtualListRef = {
  scrollTo: (config: {
    key: string;
    align?: 'top' | 'bottom' | 'auto';
    offset?: number;
  }) => void;
  focusKey: (key: string) => void;
};
type Props<T> = {
  items: T[];
  getKey: (item: T) => string;
  getEventKey?: (item: T) => string | undefined;
  renderItem: (item: T, index: number) => ReactNode;
  initialKey?: string;
  resetKey?: string;
  active?: boolean;
  className?: string;
  onVisibleKeys: (keys: string[]) => void;
};
function VirtualList<T>(
  {
    items,
    getKey,
    getEventKey,
    renderItem,
    initialKey,
    resetKey = '',
    active = true,
    className,
    onVisibleKeys
  }: Props<T>,
  ref: ForwardedRef<TrajectoryVirtualListRef>
) {
  const root = useRef<HTMLDivElement>(null);
  const list = useRef<ListyRef>(null);
  const [height, setHeight] = useState(0);
  const frame = useRef<number | null>(null);
  const initial = useRef<string | null>(null);
  const focus = useRef<string | null>(null);
  const report = useCallback(() => {
    if (!active || !root.current) return;
    const bounds = root.current.getBoundingClientRect();
    const mounted = [
      ...root.current.querySelectorAll<HTMLElement>('[data-trajectory-index]')
    ];
    const visible = mounted.filter((element) => {
      const row = element.getBoundingClientRect();
      return row.bottom >= bounds.top && row.top <= bounds.bottom;
    });
    if (visible.length) {
      const first = Number(visible[0].dataset.trajectoryIndex);
      const last = Number(visible.at(-1)!.dataset.trajectoryIndex);
      // Prefetch one actual viewport in each direction, independent of total size.
      const count = last - first + 1;
      onVisibleKeys(
        items
          .slice(Math.max(0, first - count), last + count + 1)
          .map((item) => (getEventKey ? getEventKey(item) : getKey(item)))
          .filter((key): key is string => key !== undefined)
      );
    }
    if (focus.current) {
      const element = mounted.find(
        (row) => row.dataset.trajectoryKey === focus.current
      );
      const target = element?.querySelector<HTMLElement>('button, [tabindex]');
      if (target) {
        target.focus({ preventScroll: true });
        focus.current = null;
      }
    }
  }, [active, items, getKey, getEventKey, onVisibleKeys]);
  // Observer lifetime follows the viewport, not inline consumer callbacks.
  const latestReport = useRef(report);
  useLayoutEffect(() => {
    latestReport.current = report;
  }, [report]);
  const schedule = useCallback(() => {
    if (frame.current !== null) cancelAnimationFrame(frame.current);
    frame.current = requestAnimationFrame(() => {
      frame.current = null;
      latestReport.current();
    });
  }, []);
  useLayoutEffect(() => {
    const element = root.current;
    if (!element) return;
    const update = () => {
      setHeight(element.clientHeight);
      schedule();
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, [schedule]);
  useLayoutEffect(() => {
    const token = resetKey + ':' + initialKey;
    if (
      initialKey &&
      items.some((item) => getKey(item) === initialKey) &&
      initial.current !== token &&
      height > 0
    ) {
      initial.current = token;
      list.current?.scrollTo({ key: initialKey, align: 'top' });
    }
    schedule();
  }, [items, initialKey, resetKey, getKey, height, schedule]);
  useEffect(
    () => () => {
      if (frame.current !== null) cancelAnimationFrame(frame.current);
    },
    []
  );
  useImperativeHandle(
    ref,
    () => ({
      scrollTo: (config) => {
        list.current?.scrollTo(config);
        schedule();
      },
      focusKey: (key) => {
        focus.current = key;
        list.current?.scrollTo({ key, align: 'auto' });
        schedule();
      }
    }),
    [schedule]
  );
  return (
    <div
      ref={root}
      className={['trajectory-virtual-list', className]
        .filter(Boolean)
        .join(' ')}
    >
      <Listy<T>
        ref={list}
        virtual
        height={Math.max(1, height)}
        items={items}
        rowKey={getKey}
        onScroll={schedule}
        styles={{
          root: { border: 0, borderRadius: 0 },
          item: { padding: 0, border: 0 }
        }}
        itemRender={(item, index) => (
          <div data-trajectory-index={index} data-trajectory-key={getKey(item)}>
            {renderItem(item, index)}
          </div>
        )}
      />
    </div>
  );
}
export const TrajectoryVirtualList = forwardRef(VirtualList) as <T>(
  props: Props<T> & RefAttributes<TrajectoryVirtualListRef>
) => ReactNode;
