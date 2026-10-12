import './listy-layout';
import { createRef } from 'react';
import { act, render, waitFor } from '@testing-library/react';
import { expect, test, vi } from 'vitest';
import {
  TrajectoryVirtualList,
  type TrajectoryVirtualListRef
} from '../../../components/debug-console/trajectory/virtual/TrajectoryVirtualList';

test('consumer rerenders retain the viewport observer and report current rows without mounting the full index', async () => {
  const observers: Array<{
    targets: Element[];
    callback: ResizeObserverCallback;
    disconnect: ReturnType<typeof vi.fn>;
  }> = [];
  const previousObserver = globalThis.ResizeObserver;
  globalThis.ResizeObserver = class {
    targets: Element[] = [];
    disconnect = vi.fn();
    unobserve = vi.fn();
    constructor(public callback: ResizeObserverCallback) {
      observers.push(this);
    }
    observe(target: Element) {
      this.targets.push(target);
    }
  };
  const items = Array.from({ length: 10000 }, (_, index) => ({
    id: String(index)
  }));
  const key = (row: { id: string }) => row.id;
  const firstVisible = vi.fn();
  const nextVisible = vi.fn();
  const ref = createRef<TrajectoryVirtualListRef>();
  const element = (onVisibleKeys: (keys: string[]) => void, active = true) => (
    <TrajectoryVirtualList
      ref={ref}
      items={items}
      getKey={key}
      onVisibleKeys={onVisibleKeys}
      active={active}
      renderItem={(row) => <button>{row.id}</button>}
    />
  );
  const view = render(element(firstVisible));
  try {
    await waitFor(() => expect(firstVisible).toHaveBeenCalled());
    const viewport = view.container.querySelector(
      '.trajectory-virtual-list'
    )!;
    const owner = observers.find((observer) =>
      observer.targets.includes(viewport)
    )!;
    expect(
      viewport.querySelectorAll('[data-trajectory-index]').length
    ).toBeLessThan(30);
    view.rerender(element(nextVisible));
    act(() => owner.callback([], {} as ResizeObserver));
    await waitFor(() => expect(nextVisible).toHaveBeenCalled());
    expect(owner.disconnect).not.toHaveBeenCalled();
    expect(
      observers.filter((observer) => observer.targets.includes(viewport))
    ).toHaveLength(1);
    view.rerender(element(nextVisible, false));
    nextVisible.mockClear();
    act(() => owner.callback([], {} as ResizeObserver));
    await act(() => new Promise((resolve) => requestAnimationFrame(resolve)));
    expect(nextVisible).not.toHaveBeenCalled();
    view.rerender(element(nextVisible));
    act(() => ref.current?.focusKey('0'));
    await waitFor(() => expect(document.activeElement?.textContent).toBe('0'));
  } finally {
    view.unmount();
    globalThis.ResizeObserver = previousObserver;
  }
});
