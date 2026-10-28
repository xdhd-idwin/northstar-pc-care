import { contextBridge, ipcRenderer } from 'electron';
import type { Bridge } from '../src/shared/types';
const api: Bridge = {
  getMetrics: () => ipcRenderer.invoke('metrics'),
  getSnapshots: () => ipcRenderer.invoke('snapshots'),
  runTweak: (id, enable) => ipcRenderer.invoke('tweak', { id, enable }),
  runTweakBatch: (ids) => ipcRenderer.invoke('tweak-batch', ids),
  revertAll: () => ipcRenderer.invoke('revert-all'),
  getStartup: () => ipcRenderer.invoke('startup-scan'),
  setStartup: (id, enable) => ipcRenderer.invoke('startup-set', { id, enable }),
  getScheduledStartup: () => ipcRenderer.invoke('scheduled-startup-scan'),
  setScheduledStartup: (id, enable) => ipcRenderer.invoke('scheduled-startup-set', { id, enable }),
  getDnsAdapters: () => ipcRenderer.invoke('dns-scan'),
  setDns: (index, provider) => ipcRenderer.invoke('dns-set', { index, provider }),
  restoreDns: (index) => ipcRenderer.invoke('dns-restore', { index }),
  flushDns: () => ipcRenderer.invoke('dns-flush'),
  scanJunk: () => ipcRenderer.invoke('clean-scan'),
  cleanJunk: (paths) => ipcRenderer.invoke('clean-run', paths)
};
contextBridge.exposeInMainWorld('northstar', api);
