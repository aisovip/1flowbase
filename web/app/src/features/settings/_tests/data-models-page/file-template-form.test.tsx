import '../../../../test/fixtures/rc-util-unique-ids';
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor
} from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { DataModelFormDrawer } from '../../components/data-models/DataModelFormDrawer';
import { fetchSettingsFileStorages } from '../../api/file-management';
import { resetAuthStore, useAuthStore } from '../../../../state/auth-store';

vi.mock('../../api/file-management', () => ({
  fetchSettingsFileStorages: vi.fn()
}));
const template = (template_code: string) => ({
  template_provider: 'core',
  template_code,
  template_version: 'v1',
  summary: template_code,
  description: '',
  system_fields: []
});
beforeEach(() => {
  vi.clearAllMocks();
  useAuthStore.setState({
    actor: {
      id: 'root-1',
      account: 'root',
      effective_display_role: 'root',
      current_workspace_id: 'workspace-1'
    }
  });
  vi.mocked(fetchSettingsFileStorages).mockResolvedValue([
    {
      id: 'storage-1',
      code: 'archive',
      title: '归档存储',
      driver_type: 'local',
      enabled: true,
      is_default: false,
      health_status: 'ready',
      last_health_error: null,
      config_json: {},
      rule_json: {}
    }
  ]);
});
afterEach(() => {
  resetAuthStore();
});

async function form() {
  const onCreate = vi.fn();
  render(
    <DataModelFormDrawer
      open
      mode="create"
      model={null}
      source={null}
      compatibleTemplates={[template('general'), template('file')]}
      templatesLoading={false}
      templatesError={null}
      saving={false}
      onClose={vi.fn()}
      onCreate={onCreate}
      onUpdate={vi.fn()}
    />
  );
  fireEvent.mouseDown(
    screen.getByRole('combobox', { name: 'Data Model 模板' })
  );
  fireEvent.click(await screen.findByText('文件表'));
  fireEvent.change(screen.getByLabelText('标题'), {
    target: { value: '资料' }
  });
  fireEvent.change(screen.getByLabelText('Code'), {
    target: { value: 'documents' }
  });
  return onCreate;
}

test('file template sends null for following the default storage', async () => {
  const onCreate = await form();
  expect(await screen.findByText('跟随默认存储')).toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: '创建' }));
  await waitFor(() =>
    expect(onCreate).toHaveBeenCalledWith(
      expect.objectContaining({
        template_code: 'file',
        bound_storage_id: null,
        code: 'documents'
      })
    )
  );
});
test('file template passes the selected storage ID without changing its field name', async () => {
  const onCreate = await form();
  await waitFor(() => expect(fetchSettingsFileStorages).toHaveBeenCalled());
  fireEvent.mouseDown(screen.getByRole('combobox', { name: '绑定存储' }));
  fireEvent.click(await screen.findByText('归档存储 (archive)'));
  fireEvent.click(screen.getByRole('button', { name: '创建' }));
  await waitFor(() =>
    expect(onCreate).toHaveBeenCalledWith(
      expect.objectContaining({
        template_code: 'file',
        bound_storage_id: 'storage-1'
      })
    )
  );
});

test('loading storage choices preserves title and code already entered by the user', async () => {
  let resolveStorages!: (
    items: Awaited<ReturnType<typeof fetchSettingsFileStorages>>
  ) => void;
  vi.mocked(fetchSettingsFileStorages).mockReturnValue(
    new Promise((resolve) => {
      resolveStorages = resolve;
    })
  );
  const onCreate = await form();
  await act(async () => {
    resolveStorages([]);
  });
  expect(screen.getByLabelText('标题')).toHaveValue('资料');
  expect(screen.getByLabelText('Code')).toHaveValue('documents');
  fireEvent.click(screen.getByRole('button', { name: '创建' }));
  await waitFor(() =>
    expect(onCreate).toHaveBeenCalledWith(
      expect.objectContaining({
        title: '资料',
        code: 'documents',
        bound_storage_id: null
      })
    )
  );
});
