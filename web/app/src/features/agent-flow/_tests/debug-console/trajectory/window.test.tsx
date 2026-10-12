import { act, renderHook, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import type { ReactNode } from 'react';
import { expect, test, vi } from 'vitest';
import { useTrajectoryWindow } from '../../../components/debug-console/trajectory/virtual/use-trajectory-window';

function fixture() {
  const rows = Array.from({ length: 10000 }, (_, n) => ({ id: String(n) }));
  const loadPage = vi.fn(
    async (focus: string | undefined, indexOnly: boolean) => ({
      items: indexOnly
        ? []
        : rows.slice(Number(focus ?? 0), Number(focus ?? 0) + 20),
      overview: indexOnly
        ? {
            items: rows,
            total_count: rows.length,
            category_counts: {},
            time_start: null,
            time_end: null
          }
        : undefined
    })
  );
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } }
  });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  const hook = renderHook(
    ({ scope, active }) =>
      useTrajectoryWindow({
        queryKey: ['window-fixture', scope],
        enabled: active,
        loadPage,
        getRowKey: (row: { id: string }) => row.id,
        getIndexKey: (row: { id: string }) => row.id
      }),
    { wrapper, initialProps: { scope: 'a', active: true } }
  );
  return { ...hook, loadPage, client };
}
test('complete index does not drain rows; distant viewport fetches directly and cached revisits are idle', async () => {
  const { result, loadPage, unmount, client } = fixture();
  await waitFor(() => expect(result.current.loaded.size).toBe(20));
  await act(() => new Promise((resolve) => setTimeout(resolve, 250)));
  expect(loadPage).toHaveBeenCalledTimes(2);
  expect(result.current.overview.data?.overview?.total_count).toBe(10000);
  act(() => result.current.onVisibleKeys(['9000', '9001']));
  await waitFor(() => expect(result.current.loaded.has('9001')).toBe(true));
  expect(loadPage).toHaveBeenLastCalledWith('9000', false);
  act(() => result.current.onVisibleKeys(['0', '1']));
  await act(() => new Promise((resolve) => setTimeout(resolve, 150)));
  expect(loadPage).toHaveBeenCalledTimes(3);
  unmount();
  client.clear();
});
test('inactive and hidden views pause window reads; errors require explicit retry', async () => {
  const { result, loadPage, rerender, unmount, client } = fixture();
  await waitFor(() => expect(result.current.loaded.size).toBe(20));
  rerender({ scope: 'a', active: false });
  act(() => result.current.onVisibleKeys(['9000']));
  await act(() => new Promise((resolve) => setTimeout(resolve, 150)));
  expect(loadPage).toHaveBeenCalledTimes(2);
  const visibility = vi
    .spyOn(document, 'visibilityState', 'get')
    .mockReturnValue('hidden');
  act(() => document.dispatchEvent(new Event('visibilitychange')));
  rerender({ scope: 'a', active: true });
  await act(() => new Promise((resolve) => setTimeout(resolve, 150)));
  expect(loadPage).toHaveBeenCalledTimes(2);
  loadPage.mockRejectedValueOnce(new Error('offline'));
  visibility.mockReturnValue('visible');
  act(() => document.dispatchEvent(new Event('visibilitychange')));
  await waitFor(() => expect(result.current.pages.isError).toBe(true));
  await act(() => new Promise((resolve) => setTimeout(resolve, 200)));
  expect(loadPage).toHaveBeenCalledTimes(3);
  act(() => result.current.retry());
  await waitFor(() => expect(result.current.loaded.has('9000')).toBe(true));
  expect(loadPage).toHaveBeenCalledTimes(4);
  visibility.mockRestore();
  unmount();
  client.clear();
});
test('switching scope clears extra rows and ignores a late previous-scope window', async () => {
  const { result, loadPage, rerender, unmount, client } = fixture();
  await waitFor(() => expect(result.current.loaded.size).toBe(20));
  let finish!: (page: Awaited<ReturnType<typeof loadPage>>) => void;
  loadPage.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      })
  );
  act(() => result.current.onVisibleKeys(['9000']));
  await waitFor(() => expect(loadPage).toHaveBeenCalledTimes(3));
  rerender({ scope: 'b', active: true });
  await waitFor(() => expect(loadPage).toHaveBeenCalledTimes(5));
  await act(async () =>
    finish({ items: [{ id: '9000' }], overview: undefined })
  );
  expect(result.current.loaded.has('9000')).toBe(false);
  unmount();
  client.clear();
});
