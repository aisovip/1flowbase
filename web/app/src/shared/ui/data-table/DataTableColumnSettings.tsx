import { useId, useRef, useState } from 'react';
import { Button, Checkbox, ConfigProvider, Empty, Input, Popover } from 'antd';
import type { ComponentRef } from 'react';
import TableOutlined from '@ant-design/icons/es/icons/TableOutlined';
import DownOutlined from '@ant-design/icons/es/icons/DownOutlined';
import ReloadOutlined from '@ant-design/icons/es/icons/ReloadOutlined';
import { i18nText } from '../../i18n/text';
import {
  getDefaultColumnWidths,
  getDefaultVisibleKeys,
  normalizeVisibleKeys,
  type DataTableColumn,
  type DataTableConfiguration
} from './data-table-state';
import './data-table-column-settings.css';

export function DataTableColumnSettings<T extends object>({
  ariaLabel = i18nText('sharedUi', 'auto.field_configuration'),
  className,
  columns,
  configuration,
  placeholder = i18nText('sharedUi', 'table.columns'),
  resetLabel = i18nText('sharedUi', 'auto.reset_default_fields')
}: {
  ariaLabel?: string;
  className?: string;
  columns: Array<DataTableColumn<T>>;
  configuration: DataTableConfiguration;
  placeholder?: string;
  resetLabel?: string;
}) {
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState('');
  const trigger = useRef<ComponentRef<typeof Button>>(null);
  const panelId = useId();
  const selected = normalizeVisibleKeys(
    columns,
    configuration.visibleColumnKeys
  );
  const filtered = columns.filter((column) =>
    column.title.toLocaleLowerCase().includes(search.trim().toLocaleLowerCase())
  );
  function close() {
    setOpen(false);
    trigger.current?.focus();
  }
  return (
    <Popover
      trigger="click"
      placement="bottomLeft"
      arrow={false}
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (next) setSearch('');
      }}
      styles={{ container: { padding: 0 } }}
      content={
        <div
          id={panelId}
          role="dialog"
          aria-label={i18nText('sharedUi', 'table.customize')}
          className="data-table-columns"
          onKeyDown={(event) => {
            if (event.key === 'Escape') {
              event.stopPropagation();
              close();
            }
          }}
        >
          <div className="data-table-columns__heading">
            <strong>{i18nText('sharedUi', 'table.customize')}</strong>
            <span>
              {i18nText('sharedUi', 'table.selected', {
                count: selected.length
              })}
            </span>
          </div>
          <div className="data-table-columns__search">
            <Input
              allowClear
              aria-label={i18nText('sharedUi', 'table.search')}
              placeholder={i18nText('sharedUi', 'table.search')}
              value={search}
              onChange={(event) => setSearch(event.target.value)}
            />
          </div>
          <ConfigProvider
            theme={{ components: { Checkbox: { borderRadiusSM: 4 } } }}
          >
            <div className="data-table-columns__list">
              {filtered.map((column) => (
                <Checkbox
                  className="data-table-columns__option"
                  key={column.key}
                  checked={selected.includes(column.key)}
                  disabled={
                    selected.length === 1 && selected.includes(column.key)
                  }
                  onChange={({ target }) => {
                    configuration.setVisibleColumnKeys(
                      columns
                        .filter((item) =>
                          item.key === column.key
                            ? target.checked
                            : selected.includes(item.key)
                        )
                        .map((item) => item.key)
                    );
                  }}
                >
                  {column.title}
                </Checkbox>
              ))}
              {!filtered.length && (
                <Empty
                  image={Empty.PRESENTED_IMAGE_SIMPLE}
                  description={i18nText('sharedUi', 'table.no_fields')}
                />
              )}
            </div>
          </ConfigProvider>
          <div className="data-table-columns__footer">
            <Button
              type="text"
              size="small"
              icon={<ReloadOutlined aria-hidden="true" />}
              onClick={() => {
                configuration.setVisibleColumnKeys(
                  getDefaultVisibleKeys(columns)
                );
                configuration.setColumnWidths(getDefaultColumnWidths(columns));
                setSearch('');
              }}
            >
              {resetLabel}
            </Button>
            <span>{i18nText('sharedUi', 'table.keep_one')}</span>
          </div>
        </div>
      }
    >
      <Button
        ref={trigger}
        aria-label={ariaLabel}
        aria-expanded={open}
        aria-controls={panelId}
        aria-haspopup="dialog"
        icon={<TableOutlined />}
        className={['data-table-columns__trigger', className]
          .filter(Boolean)
          .join(' ')}
      >
        {placeholder}
        <span className="data-table-columns__count">
          {selected.length}/{columns.length}
        </span>
        <DownOutlined rotate={open ? 180 : 0} />
      </Button>
    </Popover>
  );
}
