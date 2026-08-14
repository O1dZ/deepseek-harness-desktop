import { invoke } from '@tauri-apps/api/core';
import './styles.css';

const elements = {
  badge: document.querySelector('#edition-badge'),
  title: document.querySelector('#status-title'),
  detail: document.querySelector('#status-detail'),
  spinner: document.querySelector('#spinner'),
  workspaceCard: document.querySelector('#workspace-card'),
  errorCard: document.querySelector('#error-card'),
  errorMessage: document.querySelector('#error-message'),
  workspaceLabel: document.querySelector('#workspace-label'),
  runtimeLabel: document.querySelector('#runtime-label'),
};

const setVisible = (element, visible) => element.classList.toggle('hidden', !visible);

function render(state) {
  elements.badge.textContent = state.edition;
  elements.workspaceLabel.textContent = state.workspace || '尚未选择 Workspace';
  elements.runtimeLabel.textContent = state.runtimeSource || state.status;

  const needsWorkspace = state.status === 'needsWorkspace';
  const failed = state.status === 'error' || state.status === 'crashed';
  const loading = ['starting', 'checking', 'restarting'].includes(state.status);

  setVisible(elements.workspaceCard, needsWorkspace);
  setVisible(elements.errorCard, failed);
  setVisible(elements.spinner, loading);

  if (needsWorkspace) {
    elements.title.textContent = '选择一个 Workspace';
    elements.detail.textContent = '首次启动需要选择本地文件夹；之后会自动恢复。';
  } else if (failed) {
    elements.title.textContent = state.status === 'crashed' ? 'Harness Runtime 已停止' : '无法启动 Harness';
    elements.detail.textContent = '可以查看诊断信息、打开日志或重新启动。';
    elements.errorMessage.textContent = state.detail || '没有更多错误信息。';
  } else if (state.status === 'ready') {
    elements.title.textContent = 'Harness 已就绪';
    elements.detail.textContent = '正在打开工作台…';
  } else {
    elements.title.textContent = state.status === 'restarting' ? '正在恢复 Runtime' : '正在启动 Harness';
    elements.detail.textContent = state.detail || '首次启动 Lite Edition 时可能需要下载 npm 依赖。';
  }
}

async function refresh() {
  try {
    render(await invoke('get_shell_state'));
  } catch (error) {
    render({
      edition: 'Desktop',
      status: 'error',
      detail: String(error),
      workspace: null,
      runtimeSource: null,
    });
  }
}

document.querySelector('#choose-workspace').addEventListener('click', async () => {
  await invoke('choose_workspace');
  await refresh();
});

document.querySelector('#retry-runtime').addEventListener('click', async () => {
  await invoke('restart_runtime');
  await refresh();
});

document.querySelector('#open-logs').addEventListener('click', () => invoke('open_logs'));
document.querySelector('#open-settings').addEventListener('click', () => invoke('open_desktop_settings'));

await refresh();
setInterval(refresh, 750);
