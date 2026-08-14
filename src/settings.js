import { invoke } from '@tauri-apps/api/core';
import './styles.css';
import './settings.css';

let settings;
const $ = (selector) => document.querySelector(selector);

async function load() {
  const [nextSettings, state] = await Promise.all([
    invoke('get_desktop_settings'),
    invoke('get_shell_state'),
  ]);
  settings = nextSettings;
  $('#settings-edition').textContent = `${state.edition} · ${state.appVersion}`;
  $('#workspace').value = settings.workspace || '';
  $('#custom-node').value = settings.customNode || '';
  $('#custom-dsh').value = settings.customDsh || '';
  $('#allow-unverified').checked = settings.allowUnverifiedRuntime;
  $('#launch-at-login').checked = settings.launchAtLogin;
  $('#runtime-summary').textContent = `${state.status} · ${state.runtimeSource || '等待 Runtime'} · Node ${state.nodeVersion || '未知'}`;
  $('#lite-settings').classList.toggle('hidden', state.edition !== 'Lite');
}

$('#settings-choose-workspace').addEventListener('click', async () => {
  await invoke('choose_workspace');
  await load();
});

$('#settings-open-logs').addEventListener('click', () => invoke('open_logs'));
$('#settings-clear-logs').addEventListener('click', async () => {
  await invoke('clear_logs');
  $('#settings-message').textContent = '日志已清除';
});
$('#settings-restart').addEventListener('click', async () => {
  $('#settings-message').textContent = '正在重启…';
  await invoke('restart_runtime');
  $('#settings-message').textContent = '已请求重启';
});

$('#save-settings').addEventListener('click', async () => {
  settings.customNode = $('#custom-node').value.trim() || null;
  settings.customDsh = $('#custom-dsh').value.trim() || null;
  settings.allowUnverifiedRuntime = $('#allow-unverified').checked;
  settings.launchAtLogin = $('#launch-at-login').checked;
  $('#settings-message').textContent = '正在保存…';
  await invoke('save_desktop_settings', { settings });
  $('#settings-message').textContent = '设置已保存';
  await load();
});

await load();
