import { Spin } from 'antd';
import type { ClientTrajectoryIndexItem } from '@1flowbase/api-client';
import { formatDateTime } from '../../../../../../shared/i18n/format';
import { i18nText } from '../../../../../../shared/i18n/text';
import { categoryLabel } from './presentation';

export function indexLane(step: Pick<ClientTrajectoryIndexItem, 'category'>) {
  return step.category.startsWith('tool')
    ? 'tool'
    : ['request', 'user', 'system'].includes(step.category)
      ? 'input'
      : 'model';
}

/** A virtual slot holds only the authorized index until its row window arrives. */
export function PendingClientRow({
  step,
  selected,
  onSelect
}: {
  step: ClientTrajectoryIndexItem;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      className="provider-trajectory__row client-trajectory__row conversation-trajectory__row"
      data-lane={indexLane(step)}
      data-selected={selected || undefined}
      aria-pressed={selected}
      aria-busy="true"
      aria-label={`${categoryLabel(step.category)} #${step.sequence}`}
      onClick={onSelect}
    >
      <span className="provider-trajectory__marker" aria-hidden="true" />
      <span className="provider-trajectory__kind">
        {categoryLabel(step.category)}
      </span>
      <span className="client-trajectory__summary" role="status">
        <Spin size="small" />
        <span className="provider-trajectory__preview">
          {i18nText('agentFlow', 'trajectory.loading_first_page')}
        </span>
      </span>
      <span
        className="provider-trajectory__row-time"
        title={formatDateTime(step.created_at)}
      >
        #{step.sequence}
      </span>
    </button>
  );
}

export const clientRowKey = (row: { id: string }) => row.id;
export const virtualClientKey = (slot: { key: string }) => slot.key;
export const virtualClientEventKey = (slot: {
  kind: string;
  step?: { id: string };
  root?: { id: string };
}) =>
  slot.kind === 'step'
    ? slot.step?.id
    : slot.kind === 'request'
      ? slot.root?.id
      : undefined;
