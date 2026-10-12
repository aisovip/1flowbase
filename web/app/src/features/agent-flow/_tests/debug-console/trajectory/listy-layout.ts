import { beforeEach, vi } from 'vitest';

/** Deterministic geometry for real Listy and its viewport-demand reporting. */
const originalHeight = Object.getOwnPropertyDescriptor(
  HTMLElement.prototype,
  'clientHeight'
)?.get;
const originalWidth = Object.getOwnPropertyDescriptor(
  HTMLElement.prototype,
  'clientWidth'
)?.get;
const originalOffsetHeight = Object.getOwnPropertyDescriptor(
  HTMLElement.prototype,
  'offsetHeight'
)?.get;
const originalRect = HTMLElement.prototype.getBoundingClientRect;
beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockImplementation(
    function (this: HTMLElement) {
      return this.closest('.trajectory-virtual-list')
        ? 320
        : (originalHeight?.call(this) ?? 800);
    }
  );
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockImplementation(
    function (this: HTMLElement) {
      return this.closest('.provider-trajectory')
        ? 800
        : (originalWidth?.call(this) ?? 1280);
    }
  );
  vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockImplementation(
    function (this: HTMLElement) {
      return this.closest('[data-trajectory-index]')
        ? 32
        : this.closest('.trajectory-virtual-list')
          ? 320
          : (originalOffsetHeight?.call(this) ?? 800);
    }
  );
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(
    function (this: HTMLElement) {
      const row = this.closest<HTMLElement>('[data-trajectory-index]');
      const viewport = this.closest<HTMLElement>('.trajectory-virtual-list');
      if (!viewport) return originalRect.call(this);
      const scroll =
        viewport.querySelector<HTMLElement>('.ant-listy-holder')?.scrollTop ??
        0;
      const top = row ? Number(row.dataset.trajectoryIndex) * 32 - scroll : 0;
      const height = row ? 32 : 320;
      return {
        x: 0,
        y: top,
        top,
        left: 0,
        right: 800,
        bottom: top + height,
        width: 800,
        height,
        toJSON: () => ({})
      } as DOMRect;
    }
  );
});
