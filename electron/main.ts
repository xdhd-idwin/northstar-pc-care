import { app, BrowserWindow, dialog, ipcMain, Menu, shell } from 'electron';
import { autoUpdater } from 'electron-updater';
import { spawn } from 'node:child_process';
import { promises as fs } from 'node:fs';
import path from 'node:path';
import { catalog } from '../src/shared/catalog';

const isDev = !app.isPackaged;
const scriptPath = () => app.isPackaged ? path.join(process.resourcesPath,'powershell','SystemOps.ps1') : path.join(app.getAppPath(),'electron','powershell','SystemOps.ps1');
const updateSourcePath = () => app.isPackaged ? path.join(process.resourcesPath,'update-source.json') : path.join(app.getAppPath(),'electron','update-source.json');
const billingSourcePath = () => app.isPackaged ? path.join(process.resourcesPath,'billing-source.json') : path.join(app.getAppPath(),'electron','billing-source.json');
const dataDir = () => path.join(process.env.LOCALAPPDATA || app.getPath('userData'), 'NorthstarPcCare');
const freeTweakLimit = 15;
let installIdPromise:Promise<string>|undefined;
Menu.setApplicationMenu(null);

async function getBillingConfig():Promise<{baseUrl:string}> {
  try {
    const value=JSON.parse(await fs.readFile(billingSourcePath(),'utf8')) as {baseUrl?:unknown};
    const baseUrl=typeof value.baseUrl==='string'?value.baseUrl.trim().replace(/\/$/,''):'';
    if(baseUrl && !/^https:\/\//i.test(baseUrl) && !/^http:\/\/127\.0\.0\.1(?::\d+)?$/i.test(baseUrl)) return {baseUrl:''};
    return {baseUrl};
  } catch { return {baseUrl:''}; }
}
async function getInstallId():Promise<string> {
  if(!installIdPromise) installIdPromise=(async()=>{
    const dir=await dataDir();await fs.mkdir(dir,{recursive:true});const file=path.join(dir,'billing-install-id');
    try { const saved=(await fs.readFile(file,'utf8')).trim();if(/^[0-9a-f-]{36}$/i.test(saved))return saved; } catch {}
    const {randomUUID}=await import('node:crypto');const id=randomUUID();await fs.writeFile(file,id,{encoding:'utf8',flag:'wx'}).catch(async()=>{});return id;
  })();
  return installIdPromise;
}
async function billingRequest(route:string,init?:RequestInit):Promise<any> {
  const {baseUrl}=await getBillingConfig();if(!baseUrl)throw new Error('Northstar billing is not configured yet.');
  const response=await fetch(`${baseUrl}${route}`,{...init,headers:{'content-type':'application/json',...(init?.headers||{})},signal:AbortSignal.timeout(12000)});
  const result=await response.json().catch(()=>({}));if(!response.ok)throw new Error(typeof result.error==='string'?result.error:`Billing service returned ${response.status}.`);return result;
}
async function getProEntitlement():Promise<{configured:boolean;pro:boolean;plan:string|null;error?:string}> {
  const {baseUrl}=await getBillingConfig();
  if(!baseUrl)return {configured:false,pro:false,plan:null};
  try { const installId=await getInstallId();const result=await billingRequest(`/v1/entitlement/${encodeURIComponent(installId)}`);return {configured:true,pro:result.pro===true,plan:result.plan??null}; }
  catch(error) { return {configured:true,pro:false,plan:null,error:String(error)}; }
}

function createWindow() {
  const win = new BrowserWindow({ width: 1320, height: 860, minWidth: 1050, minHeight: 700, backgroundColor:'#0b0f17', title:'Northstar PC Care', autoHideMenuBar:true, webPreferences:{ preload:path.join(__dirname,'preload.js'), contextIsolation:true, nodeIntegration:false, sandbox:true } });
  win.webContents.session.webRequest.onHeadersReceived((d, cb) => cb({responseHeaders:{...d.responseHeaders,'Content-Security-Policy':["default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'self' http://127.0.0.1:5173 ws://127.0.0.1:5173"]}}));
  if(isDev) void win.loadURL('http://127.0.0.1:5173'); else void win.loadFile(path.join(app.getAppPath(),'dist/index.html'));
}

type Reply = {ok:boolean; message:string; code?:string; data?:unknown};
async function getEnabledTweakIds():Promise<Set<string>> {
  const result=await runPs({Action:'Snapshots'});
  if(!result.ok) throw new Error(result.message);
  const snapshots=Array.isArray(result.data)?result.data:result.data?[result.data]:[];
  const catalogIds=new Set(catalog.map(item=>item.id));
  return new Set(snapshots.filter((snapshot:any)=>snapshot?.enabled&&catalogIds.has(snapshot.id)).map((snapshot:any)=>String(snapshot.id)));
}
async function askForRestorePoint(count:number):Promise<boolean|null> {
  const title = count === 1 ? 'Before applying this tweak' : `Before applying ${count} tweaks`;
  const options = {
    type: 'question' as const,
    title: 'Create a Windows restore point?',
    message: title,
    detail: 'A Windows restore point can help roll back system changes, but Windows may have System Protection disabled. Northstar will also save the exact original settings so you can revert these changes in the app.',
    buttons: ['Create restore point and apply', 'Apply without restore point', 'Cancel'],
    defaultId: 0,
    cancelId: 2,
    noLink: true,
  };
  const parent = BrowserWindow.getFocusedWindow();
  const { response } = parent ? await dialog.showMessageBox(parent, options) : await dialog.showMessageBox(options);
  if (response === 2) return null;
  return response === 0;
}

async function runPs(payload:unknown):Promise<Reply> {
  const base=await fs.mkdtemp(path.join(app.getPath('temp'),'northstar-'));
  const input=path.join(base,'request.json'), output=path.join(base,'response.json');
  await fs.writeFile(input,JSON.stringify({ ...payload as object, DataDir: await dataDir(), OutputPath:output }),{encoding:'utf8',flag:'wx'});
  const args=['-NoLogo','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',scriptPath(),'-PayloadPath',input];
  const run=()=>new Promise<Reply>((resolve)=>{
    const ps=spawn('powershell.exe',args,{windowsHide:true});
    let stderr=''; ps.stderr?.on('data',(x)=>stderr+=String(x));
    ps.on('error',(e)=>resolve({ok:false,message:e.message}));
    ps.on('close',async(code)=>{
      try { const r=JSON.parse(await fs.readFile(output,'utf8')) as Reply; resolve(r); }
      catch { resolve({ok:false,message:stderr.trim()|| (code===null?'PowerShell was interrupted.':'PowerShell returned no result.'),code:code===null?'CANCELLED':'ENGINE_ERROR'}); }
    });
  });
  try {
    return await run();
  } finally { await fs.rm(base,{recursive:true,force:true}).catch(()=>{}); }
}

async function startAppWithUpdateCheck() {
  if(!app.isPackaged || process.platform!=='win32') { createWindow(); return; }
  try {
    const config=JSON.parse(await fs.readFile(updateSourcePath(),'utf8')) as {provider?:string;owner?:string;repo?:string};
    const owner=typeof config.owner==='string'?config.owner.trim():'';
    const repo=typeof config.repo==='string'?config.repo.trim():'';
    if(config.provider!=='github'||!/^[-A-Za-z0-9]+$/.test(owner)||!/^[-A-Za-z0-9_.]+$/.test(repo)) { createWindow(); return; }
    autoUpdater.setFeedURL({provider:'github',owner,repo,private:false});
    autoUpdater.autoDownload=true;
    autoUpdater.autoInstallOnAppQuit=false;
    let installing=false;
    autoUpdater.once('update-downloaded',()=>{installing=true;autoUpdater.quitAndInstall(false,true);});
    autoUpdater.on('error',error=>console.warn('Update check/download failed; opening Northstar normally.',error));
    const check=await autoUpdater.checkForUpdates();
    if(check?.isUpdateAvailable && check.downloadPromise) { await check.downloadPromise; if(installing) return; }
  } catch(error) {
    if((error as NodeJS.ErrnoException).code!=='ENOENT') console.warn('Update check failed; opening Northstar normally.',error);
  }
  createWindow();
}
app.whenReady().then(()=>{void startAppWithUpdateCheck();app.on('activate',()=>{if(BrowserWindow.getAllWindows().length===0)createWindow();});});
app.on('window-all-closed',()=>{if(process.platform!=='darwin')app.quit();});
ipcMain.handle('metrics',async()=>{const r=await runPs({Action:'Metrics'});return r.data??null;});
ipcMain.handle('billing-status',()=>getProEntitlement());
ipcMain.handle('billing-checkout',async(_event,plan:unknown)=>{
  if(plan!=='monthly'&&plan!=='lifetime')return {ok:false,message:'Invalid Pro plan.'};
  try {const installId=await getInstallId();const result=await billingRequest('/v1/checkout',{method:'POST',body:JSON.stringify({installId,plan})});if(typeof result.url!=='string'||!/^https:\/\/checkout\.stripe\.com\//i.test(result.url))return {ok:false,message:'Billing service returned an invalid checkout link.'};await shell.openExternal(result.url);return {ok:true,message:'Stripe Checkout opened in your browser.'};}
  catch(error){return {ok:false,message:String(error)};}
});
ipcMain.handle('snapshots',async()=>{const r=await runPs({Action:'Snapshots'});return Array.isArray(r.data)?r.data:r.data?[r.data]:[];});
ipcMain.handle('tweak',async(_event,arg:{id:string;enable:boolean})=>{
  const item=catalog.find(t=>t.id===arg?.id);
  const supported=!!item&&((!!item.key&&!!item.name)||(item.kind==='Service'&&!!item.serviceName)||(item.kind==='PowerPlan')||(item.kind==='PowerSetting'&&!!item.powerSetting)||(item.kind==='NetworkNagle')||(item.kind==='Hibernate')||(item.kind==='CompatibilityTasks'));
  if(!item || !item.applicable || !supported) return {ok:false,message:'This catalog item is review-only.'};
  if(typeof arg.enable!=='boolean') return {ok:false,message:'Invalid operation.'};
  if(arg.enable && item.risk==='High') return {ok:false,message:'High-risk operation is not enabled.'};
  if(arg.enable) {
    try {
      const billing=await getProEntitlement();
      const isPro=billing.pro;
      const active=await getEnabledTweakIds();
      if(!isPro&&!active.has(item.id)&&active.size>=freeTweakLimit) return {ok:false,message:`Free includes up to ${freeTweakLimit} active tweaks. Revert one before enabling another, or choose Pro.`,code:'FREE_LIMIT'};
    } catch(error) { return {ok:false,message:`Could not verify the active tweak count: ${String(error)}`}; }
  }
  let createRestorePoint=false;
  if(arg.enable) {
    const choice=await askForRestorePoint(1);
    if(choice===null) return {ok:false,message:'Cancelled. No tweak was applied.',code:'USER_CANCELLED'};
    createRestorePoint=choice;
  }
  return runPs({Action:'Tweak',Id:item.id,Enable:arg.enable,CreateRestorePoint:createRestorePoint,Tweak:{key:item.key,name:item.name,kind:item.kind,value:item.value,serviceName:item.serviceName,powerSetting:item.powerSetting,powerAc:item.powerAc,powerDc:item.powerDc}});
});
ipcMain.handle('tweak-batch',async(_event,ids:unknown)=>{
  if(!Array.isArray(ids)||ids.length<1||ids.length>20||ids.some(x=>typeof x!=='string')||new Set(ids).size!==ids.length)return {ok:false,message:'Invalid optimization profile.'};
  const items=ids.map(id=>catalog.find(t=>t.id===id));
  if(items.some(t=>!t||!t.applicable||t.kind==='Service'||t.risk!=='Low'||!t.key||!t.name))return {ok:false,message:'The profile contains an unsupported tweak.'};
  try {
    const billing=await getProEntitlement();
    const isPro=billing.pro;
    const active=await getEnabledTweakIds();
    const additional=new Set(ids.filter(id=>!active.has(id)));
    if(!isPro&&active.size+additional.size>freeTweakLimit) return {ok:false,message:`Free includes up to ${freeTweakLimit} active tweaks. Revert some active tweaks before applying this profile, or choose Pro.`,code:'FREE_LIMIT'};
  } catch(error) { return {ok:false,message:`Could not verify the active tweak count: ${String(error)}`}; }
  const safeItems=items.map(t=>({id:t!.id,tweak:{key:t!.key,name:t!.name,kind:t!.kind,value:t!.value}}));
  const choice=await askForRestorePoint(items.length);
  if(choice===null) return {ok:false,message:'Cancelled. No tweaks were applied.',code:'USER_CANCELLED'};
  return runPs({Action:'TweakBatch',CreateRestorePoint:choice,Items:safeItems});
});
ipcMain.handle('revert-all',()=>runPs({Action:'RevertAll'}));
ipcMain.handle('startup-scan',async()=>{const r=await runPs({Action:'StartupScan'});return Array.isArray(r.data)?r.data:r.data?[r.data]:[];});
ipcMain.handle('startup-set',async(_event,arg:{id:string;enable:boolean})=>{
  if(!arg||!/^\w{64}$/.test(arg.id)||typeof arg.enable!=='boolean')return {ok:false,message:'Invalid startup item.'};
  return runPs({Action:'StartupSet',Id:arg.id,Enable:arg.enable});
});
ipcMain.handle('scheduled-startup-scan',async()=>{const r=await runPs({Action:'ScheduledStartupScan'});return Array.isArray(r.data)?r.data:r.data?[r.data]:[];});
ipcMain.handle('scheduled-startup-set',async(_event,arg:{id:string;enable:boolean})=>{
  if(!arg||!/^\w{64}$/.test(arg.id)||typeof arg.enable!=='boolean')return {ok:false,message:'Invalid scheduled startup item.'};
  return runPs({Action:'ScheduledStartupSet',Id:arg.id,Enable:arg.enable});
});
ipcMain.handle('dns-scan',async()=>{const r=await runPs({Action:'DnsScan'});return Array.isArray(r.data)?r.data:r.data?[r.data]:[];});
ipcMain.handle('dns-set',async(_event,arg:{index:number;provider:string})=>{
  if(!arg||!Number.isInteger(arg.index)||arg.index<1||arg.index>65535||!['cloudflare','google'].includes(arg.provider))return {ok:false,message:'Invalid DNS adapter or provider.'};
  return runPs({Action:'DnsSet',InterfaceIndex:arg.index,Provider:arg.provider});
});
ipcMain.handle('dns-restore',async(_event,arg:{index:number})=>{
  if(!arg||!Number.isInteger(arg.index)||arg.index<1||arg.index>65535)return {ok:false,message:'Invalid DNS adapter.'};
  return runPs({Action:'DnsRestore',InterfaceIndex:arg.index});
});
ipcMain.handle('dns-flush',async()=>runPs({Action:'DnsFlush'}));
ipcMain.handle('clean-scan',async()=>[]);
ipcMain.handle('clean-run',async(_event,paths:unknown)=>{
  if(!Array.isArray(paths)||paths.length>2000||paths.some(x=>typeof x!=='string'||x.length>32767))return {ok:false,message:'Invalid cleanup selection.'};
  return {ok:false,message:'Cleaner is part of Pro. Choose a plan to see the available options.',code:'PRO_REQUIRED'};
});
