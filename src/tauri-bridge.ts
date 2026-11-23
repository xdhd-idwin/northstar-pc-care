import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Bridge, ProfileProgress } from './shared/types';

const askForRestorePoint = () => window.confirm(
  'Create a Windows restore point before applying this change?\n\nChoose OK to try a restore point, or Cancel to apply with Northstar’s per-setting backup only.'
);

export const northstarBridge: Bridge = {
  getMetrics: () => invoke('bridge', { method: 'getMetrics', args: {} }),
  getSnapshots: () => invoke('bridge', { method: 'getSnapshots', args: {} }),
  runTweak: (id, enable) => invoke('bridge', { method: 'runTweak', args: { id, enable, createRestorePoint: enable && askForRestorePoint() } }),
  runTweakBatch: (ids) => invoke('bridge', { method: 'runTweakBatch', args: { ids, createRestorePoint: askForRestorePoint() } }),
  runProfile: (ids) => invoke('bridge', { method: 'runProfile', args: { ids, createRestorePoint: askForRestorePoint() } }),
  onProfileProgress: (callback) => listen<ProfileProgress>('profile-progress', event => callback(event.payload)),
  revertAll: () => invoke('bridge', { method: 'revertAll', args: {} }),
  getStartup: () => invoke('bridge', { method: 'getStartup', args: {} }),
  setStartup: (id, enable) => invoke('bridge', { method: 'setStartup', args: { id, enable } }),
  getScheduledStartup: () => invoke('bridge', { method: 'getScheduledStartup', args: {} }),
  setScheduledStartup: (id, enable) => invoke('bridge', { method: 'setScheduledStartup', args: { id, enable } }),
  getDnsAdapters: () => invoke('bridge', { method: 'getDnsAdapters', args: {} }),
  setDns: (index, provider) => invoke('bridge', { method: 'setDns', args: { index, provider } }),
  restoreDns: (index) => invoke('bridge', { method: 'restoreDns', args: { index } }),
  flushDns: () => invoke('bridge', { method: 'flushDns', args: {} }),
  scanJunk: () => invoke('bridge', { method: 'scanJunk', args: {} }),
  cleanJunk: (paths) => invoke('bridge', { method: 'cleanJunk', args: { paths } }),
  getBillingStatus: () => invoke('bridge', { method: 'getBillingStatus', args: {} }),
  startCheckout: (plan) => invoke('bridge', { method: 'startCheckout', args: { plan } })
};

declare global {
  interface Window { northstar: Bridge }
}

window.northstar = northstarBridge;
