import type { Category, Tweak } from './types';

// Explicit, reviewable catalog. Only entries with an audited, reversible value are actionable.
const rows: Tweak[] = [];
const reviewReasons:Record<string,string>={
  'Timer resolution policy':'Timer resolution is requested by running processes through a runtime API; there is no reliable persistent global registry switch.',
  'Global fullscreen optimizations':'Windows compatibility flags are per executable and can change game behavior. The library exposes the reversible per-user game-configuration flags separately.',
  'Per-game fullscreen optimizations':'Requires selecting and backing up a specific game executable compatibility entry.',
  'Disable Windows key':'Requires a keyboard-wide scancode map and restart, which can disable essential shortcuts across all apps.',
  'GPU driver low-latency mode':'Driver controls differ across NVIDIA, AMD, and Intel and are not exposed through one stable Windows setting.',
  'Per-game network priority':'Needs game-specific process or traffic rules and can interfere with anti-cheat or VPN routing.',
  'Raw input policy':'Raw input is selected by applications; there is no supported system-wide toggle.',
  'Variable refresh rate policy':'Depends on GPU, display, driver, and per-app capability.',
  'Disable CPU C-states':'C-states are firmware and processor controls; changing them generically can raise heat and power use.',
  'Disable processor idle states':'Processor idle states are platform-managed firmware behavior and are not safely controlled as a generic Windows toggle.',
  'Disable dynamic tick':'Changes boot configuration and can disrupt timers, power management, or virtualization.',
  'Disable HPET':'Boot-level timer selection varies by platform and can reduce performance or stability.',
  'Disable CPU frequency scaling':'Windows and processor firmware jointly manage frequency scaling; min/max state controls are available instead.',
  'Disable thermal throttling':'Thermal protection must remain enabled to protect the CPU and system.',
  'Set DNS over HTTPS':'Resolver and DoH support vary by Windows build and adapter; the current DNS selector changes ordinary DNS servers with exact restore.',
  'TCP autotuning level':'Global TCP stack changes can reduce throughput or compatibility; the default adapts to the connection.',
  'ECN capability':'ECN support depends on the network path and can affect connectivity.',
  'RSS queue tuning':'Queue counts and processor affinity depend on the installed network adapter and driver.',
  'Disable QoS reservation':'There is no universal bandwidth reservation switch that improves latency across Windows versions.',
  'Disable Teredo':'Can break Xbox party/chat and applications that rely on Teredo IPv6 tunneling.',
  'Disable ISATAP':'Can break networks that rely on ISATAP IPv6 transition services.',
  'Disable 6to4':'Can break networks that rely on 6to4 IPv6 transition services.',
  'Set MTU':'Requires a per-adapter, link-specific value; an incorrect MTU can break connectivity.',
  'Disable network power saving':'Power-management controls are vendor-specific adapter settings.',
  'Set interface metric':'Needs the user to choose which adapter should take precedence; a global default would change routing.',
  'Disable IPv6 transition technology':'Would alter multiple connectivity mechanisms and can break VPN or IPv6 paths.',
  'Disable SmartScreen':'Reduces protection against malicious downloads and sites.',
  'Disable Defender telemetry':'Would change Microsoft Defender security and diagnostic behavior; security controls remain under Windows Security.',
  'Disable Search indexing on gaming drives':'Requires choosing target drives and preserving each volume’s prior indexing state.',
  'Disable Prefetch':'Prefetch can improve launch performance; disabling it is not a general optimization.',
  'Set NTFS memory usage high':'Large-system-cache mode targets server workloads and can reduce responsiveness on desktop PCs.',
  'Clear standby list':'A one-shot purge can evict useful cached data and is not a persistent optimization.',
  'Disable memory compression':'Can increase paging and memory pressure.',
  'Disable pagefile':'Can cause application failures or system instability when memory is exhausted.',
  'Optimize TRIM schedule':'Windows already manages scheduled storage optimization; custom scheduling needs drive-specific policy.',
  'Disable reserved storage':'Windows servicing owns reserved storage and may re-enable it during updates.'
};
function review(category: Category, titles: string[]) { titles.forEach((title, i) => rows.push({ id: `${category.toLowerCase().replace(/[^a-z]+/g,'-')}-${String(i+1).padStart(2,'0')}`, category, title, description: `Review-only: ${reviewReasons[title]??'This control needs device-specific validation before it can be changed safely.'}`, applicable: false, risk: 'High' })); }
function reg(category: Category, title: string, description: string, key: string, name: string, value: number|string, kind: 'DWord'|'String'='DWord', restart=false, risk: Tweak['risk']='Low') { rows.push({ id: `safe-${rows.length+1}`, category, title, description, applicable: true, risk, key, name, value, kind, restart }); }
function service(title:string,description:string,serviceName:string,category:Category) { rows.push({id:`safe-${rows.length+1}`,category,title,description,applicable:true,risk:'Medium',kind:'Service',serviceName,restart:true}); }
function powerPlan(title:string,description:string) { rows.push({id:`safe-${rows.length+1}`,category:power,title,description,applicable:true,risk:'Medium',kind:'PowerPlan'}); }
function powerSetting(title:string,description:string,powerSetting:Tweak['powerSetting'],value:number,dcValue=value) { rows.push({id:`safe-${rows.length+1}`,category:power,title,description,applicable:true,risk:'Medium',kind:'PowerSetting',powerSetting,powerAc:value,powerDc:dcValue}); }
function networkNagle(title:string,description:string) { rows.push({id:`safe-${rows.length+1}`,category:network,title,description,applicable:true,risk:'Medium',kind:'NetworkNagle'}); }
function hibernate(title:string,description:string) { rows.push({id:`safe-${rows.length+1}`,category:memory,title,description,applicable:true,risk:'Medium',kind:'Hibernate'}); }
function compatibilityTasks(title:string,description:string) { rows.push({id:`safe-${rows.length+1}`,category:privacy,title,description,applicable:true,risk:'Medium',kind:'CompatibilityTasks'}); }

const gaming: Category='Gaming & Latency', power: Category='Power & CPU', network: Category='Network & Ping', privacy: Category='Privacy & Telemetry', visual: Category='Visual & System', memory: Category='Memory & Storage';
reg(gaming,'Enable Game Mode','Sets the current-user Game Mode preference.','HKCU\\Software\\Microsoft\\GameBar','AllowAutoGameMode',1);
reg(gaming,'Disable Xbox Game Bar capture','Turns off background game capture.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\GameDVR','AppCaptureEnabled',0);
reg(gaming,'Disable Game DVR policy','Disables Game DVR capture policy for this user.','HKCU\\System\\GameConfigStore','GameDVR_Enabled',0);
reg(gaming,'Disable mouse acceleration','Uses raw pointer movement by disabling Enhance Pointer Precision.','HKCU\\Control Panel\\Mouse','MouseSpeed','0','String');
reg(gaming,'Set mouse threshold 1','Removes first Windows pointer acceleration threshold.','HKCU\\Control Panel\\Mouse','MouseThreshold1','0','String');
reg(gaming,'Set mouse threshold 2','Removes second Windows pointer acceleration threshold.','HKCU\\Control Panel\\Mouse','MouseThreshold2','0','String');
reg(gaming,'Disable background capture','Turns off background recording preference.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\GameDVR','HistoricalCaptureEnabled',0);
reg(gaming,'Set Game Bar presence writer off','Stops Game Bar presence writer behavior.','HKCU\\Software\\Microsoft\\GameBar','ShowStartupPanel',0);
reg(gaming,'Disable Game Bar tips','Hides first-run Game Bar prompts.','HKCU\\Software\\Microsoft\\GameBar','UseNexusForGameBarEnabled',0);
reg(gaming,'Disable fullscreen optimization behavior','Sets per-user compatibility behavior flags. Some legacy games may need the opposite setting.','HKCU\\System\\GameConfigStore','GameDVR_FSEBehaviorMode',2);
reg(gaming,'Honor per-user fullscreen flags','Enables Windows to honor per-user fullscreen optimization flags.','HKCU\\System\\GameConfigStore','GameDVR_HonorUserFSEBehaviorMode',1);
reg(gaming,'Disable fullscreen optimization compatibility behavior','Sets the Windows game configuration compatibility behavior flag.','HKCU\\System\\GameConfigStore','GameDVR_DXGIHonorFSEWindowsCompatible',1);
service('Disable Xbox Live Auth Manager at startup','Stops the service from starting automatically. Xbox sign-in and connected games may be affected.','XblAuthManager',gaming);
service('Disable Xbox Live Game Save at startup','Stops the service from starting automatically. Cloud saves may be affected.','XblGameSave',gaming);
service('Disable Xbox Live Networking at startup','Stops the service from starting automatically. Xbox networking features may be affected.','XboxNetApiSvc',gaming);
service('Disable Xbox Accessory service at startup','Stops the service from starting automatically. Xbox accessory features may be affected.','XboxGipSvc',gaming);
// HAGS is a documented Windows preference, but the GPU, driver and Windows build
// decide whether it takes effect. Keep it explicit and require a restart.
reg(gaming,'Request hardware-accelerated GPU scheduling','Requests HAGS where the Windows build, GPU and driver support it. The system may ignore this preference; restart to apply.','HKLM\\SYSTEM\\CurrentControlSet\\Control\\GraphicsDrivers','HwSchMode',2,'DWord',true);
reg(gaming,'Disable Sticky Keys shortcut','Disables the five-press Shift shortcut that can interrupt games. Sticky Keys accessibility remains available from Settings.','HKCU\\Control Panel\\Accessibility\\StickyKeys','Flags','506','String');
service('Disable GameInput service at startup','Stops GameInput from starting automatically. Xbox controllers and games that use newer Windows input APIs may stop working.','GameInputSvc',gaming);
review(gaming,['Timer resolution policy','Global fullscreen optimizations','Per-game fullscreen optimizations','Disable Windows key','GPU driver low-latency mode','Per-game network priority','Raw input policy','Variable refresh rate policy']);

reg(power,'Disable power throttling','Disables Windows power throttling policy. Higher power use possible.','HKLM\\SYSTEM\\CurrentControlSet\\Control\\Power\\PowerThrottling','PowerThrottlingOff',1);
reg(power,'Set system responsiveness for games','Sets MMCSS SystemResponsiveness to 10 (Windows default intent is multimedia reservation).','HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile','SystemResponsiveness',10);
reg(power,'Enable long path support','Allows long paths for aware applications.','HKLM\\SYSTEM\\CurrentControlSet\\Control\\FileSystem','LongPathsEnabled',1);
service('Set SysMain service to manual','Leaves SysMain available on demand without automatically starting it. Startup behavior varies by Windows build.','SysMain',power);
powerPlan('Activate Ultimate Performance power plan','Switches to the built-in Ultimate Performance scheme. Saves and restores the previously active plan; may increase power use and heat.');
powerSetting('Keep all CPU cores unparked','Sets the minimum unparked core percentage to 100 for the active plan. This can increase idle power and heat.','minUnparkedCores',100);
powerSetting('Allow all cores to remain unparked','Sets the maximum unparked core percentage to 100 for the active plan. This can increase idle power and heat.','maxUnparkedCores',100);
powerSetting('Set minimum processor state to 100%','Keeps the CPU at its minimum performance state while plugged in; leaves battery minimum at 5%. Restores previous AC and DC values when disabled. This can increase heat and power use.','minProcessor',100,5);
powerSetting('Set maximum processor state to 100%','Allows full processor performance on AC and battery power. Restores prior power-plan values when disabled; higher performance can increase heat and battery use.','maxProcessor',100);
powerSetting('Disable USB selective suspend','Prevents Windows from suspending idle USB devices on the active power plan. Microsoft recommends leaving this enabled for battery efficiency; turn this on only to troubleshoot device dropouts.','usbSelectiveSuspend',0);
review(power,['Disable CPU C-states','Disable processor idle states','Disable dynamic tick','Disable HPET','Disable CPU frequency scaling','Disable thermal throttling']);

reg(network,'Disable Nagle algorithm (global TCP)','Sets TCP_NODELAY only at the system TCP parameters level; per-adapter tuning is not supported here.','HKLM\\SYSTEM\\CurrentControlSet\\Services\\Tcpip\\Parameters','TcpNoDelay',1);
reg(network,'Set TCP delayed ACK frequency','Sets the system TCP delayed ACK frequency. Adapter-specific overrides may take precedence.','HKLM\\SYSTEM\\CurrentControlSet\\Services\\Tcpip\\Parameters','TcpAckFrequency',1);
reg(network,'Disable network throttling','Sets multimedia network throttling index to disabled sentinel.','HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile','NetworkThrottlingIndex',4294967295);
reg(network,'Set multimedia system profile games priority','Sets the system profile games priority to a common default multimedia setting.','HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile\\Tasks\\Games','Priority',6);
reg(network,'Set multimedia GPU priority','Sets the system profile games GPU priority.','HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile\\Tasks\\Games','GPU Priority',8);
reg(network,'Set multimedia games scheduling category','Sets the system profile games scheduling category.','HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile\\Tasks\\Games','Scheduling Category','High','String');
networkNagle('Disable Nagle algorithm on network adapters','Sets TCP_NODELAY and delayed-ACK frequency to 1 on each installed TCP/IP adapter interface. Saves the original per-adapter values; throughput or compatibility may change.');
review(network,['Set DNS over HTTPS','TCP autotuning level','ECN capability','RSS queue tuning','Disable QoS reservation','Disable Teredo','Disable ISATAP','Disable 6to4','Set MTU','Disable network power saving','Set interface metric','Disable IPv6 transition technology']);

reg(privacy,'Limit diagnostic data to required','Sets the Windows diagnostic data policy. Edition and management policy can override this.','HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\DataCollection','AllowTelemetry',1);
reg(privacy,'Disable advertising ID','Disables personalized ads for current user.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\AdvertisingInfo','Enabled',0);
reg(privacy,'Disable tailored experiences','Turns off consumer tailored experiences.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Privacy','TailoredExperiencesWithDiagnosticDataEnabled',0);
reg(privacy,'Disable feedback notifications','Stops Windows feedback prompts.','HKCU\\Software\\Microsoft\\Siuf\\Rules','NumberOfSIUFInPeriod',0);
reg(privacy,'Disable activity publishing','Disables activity feed publishing for current user.','HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\System','PublishUserActivities',0);
reg(privacy,'Disable activity upload','Disables activity upload policy.','HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\System','UploadUserActivities',0);
reg(privacy,'Disable consumer features','Turns off Windows consumer experience suggestions policy.','HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\CloudContent','DisableWindowsConsumerFeatures',1);
reg(privacy,'Disable app suggestions','Turns off suggested app notifications for current user.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\ContentDeliveryManager','SystemPaneSuggestionsEnabled',0);
reg(privacy,'Disable lock screen suggestions','Turns off lock screen content suggestions.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\ContentDeliveryManager','RotatingLockScreenEnabled',0);
reg(privacy,'Disable silent app installs','Disables suggested app installs for current user.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\ContentDeliveryManager','SilentInstalledAppsEnabled',0);
reg(privacy,'Disable advertising ID policy','Disables advertising ID policy.','HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\AdvertisingInfo','DisabledByGroupPolicy',1);
reg(privacy,'Disable Bing web search in Start','Disables web search suggestions in Start menu search.','HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\Windows Search','DisableWebSearch',1);
reg(privacy,'Disable search cloud content','Disables cloud content suggestions in Windows Search.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\SearchSettings','IsMSACloudSearchEnabled',0);
reg(privacy,'Disable work account search content','Disables organization cloud content suggestions in Search.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\SearchSettings','IsAADCloudSearchEnabled',0);
reg(privacy,'Disable recent document tracking','Stops Windows tracking recent documents for the current user.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Policies\\Explorer','NoRecentDocsHistory',1);
reg(privacy,'Disable recent item jump lists','Stops Windows tracking recent items in jump lists for the current user.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Policies\\Explorer','NoRecentDocsNetHood',1);
reg(privacy,'Disable app launch tracking','Stops Windows tracking app launches for suggestions.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced','Start_TrackProgs',0);
reg(privacy,'Disable document tracking','Stops Windows tracking recently opened documents.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced','Start_TrackDocs',0);
service('Disable Remote Registry at startup','Prevents the Remote Registry service from starting automatically; remote administration workflows may need it.','RemoteRegistry',privacy);
service('Disable Windows Error Reporting at startup','Prevents automatic start of Windows Error Reporting. Crash reporting may be unavailable.','WerSvc',privacy);
service('Disable Connected User Experiences and Telemetry at startup','Prevents the DiagTrack service from starting automatically where installed. Windows diagnostics and connected experiences may be limited.','DiagTrack',privacy);
service('Disable WAP Push Message Routing at startup','Prevents the WAP Push Message Routing service from starting automatically where installed. Mobile device management and push-message features may be affected.','dmwappushservice',privacy);
reg(privacy,'Disable Cortana policy','Applies the Windows Search policy used to disable Cortana on Windows versions that still support it.','HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\Windows Search','AllowCortana',0);
reg(privacy,'Disable location policy','Disables Windows location services by policy. Apps that rely on location may stop working.','HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\LocationAndSensors','DisableLocation',1);
reg(privacy,'Disable clipboard history policy','Disables clipboard history and cross-device clipboard features by policy.','HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\System','AllowClipboardHistory',0);
reg(privacy,'Disable cross-device clipboard policy','Disables clipboard synchronization across devices by policy.','HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\System','AllowCrossDeviceClipboard',0);
reg(privacy,'Disable online speech recognition','Turns off cloud-based speech recognition for the current user. Voice dictation and speech features that use Microsoft online speech services may stop working.','HKCU\\Software\\Microsoft\\Speech_OneCore\\Settings\\OnlineSpeechPrivacy','HasAccepted',0);
reg(privacy,'Disable OneDrive sync policy','Blocks the OneDrive sync client by Windows policy. Revert this setting before using OneDrive again.','HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\OneDrive','DisableFileSyncNGSC',1,'DWord',false,'Medium');
compatibilityTasks('Disable Windows compatibility telemetry tasks','Disables only the allowlisted Application Experience and Customer Experience scheduled tasks. Original task enabled states are saved and restored; Windows servicing may recreate them.');
review(privacy,['Disable SmartScreen','Disable Defender telemetry']);

reg(visual,'Disable transparency','Turns off transparency effects for current user.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize','EnableTransparency',0);
reg(visual,'Prefer best performance effects','Changes current-user visual effects preference.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\VisualEffects','VisualFXSetting',2);
reg(visual,'Disable taskbar animations','Turns off taskbar animations for current user.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced','TaskbarAnimations',0);
reg(visual,'Disable window minimize animations','Disables minimize/maximize animation.','HKCU\\Control Panel\\Desktop\\WindowMetrics','MinAnimate','0','String');
reg(visual,'Disable menu animations','Disables menu fade animation.','HKCU\\Control Panel\\Desktop','MenuShowDelay','0','String');
reg(visual,'Disable acrylic effects','Disables acrylic blur for current user.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize','EnableAcrylicBackgroundOnLogon',0);
reg(visual,'Disable lock screen tips','Disables Windows spotlight tips.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\ContentDeliveryManager','SubscribedContent-338387Enabled',0);
reg(visual,'Show file extensions','Shows known file extensions in Explorer.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced','HideFileExt',0);
reg(visual,'Show hidden files','Shows hidden files in Explorer.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced','Hidden',1);
reg(visual,'Disable search highlights','Turns off Search highlights.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\SearchSettings','IsDynamicSearchBoxEnabled',0);
reg(visual,'Hide Task View button','Hides the Task View button from the taskbar.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced','ShowTaskViewButton',0);
reg(visual,'Hide taskbar widgets button','Hides the Widgets button where supported.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced','TaskbarDa',0);
reg(visual,'Hide taskbar chat button','Hides the Chat button where supported.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced','TaskbarMn',0);
reg(visual,'Disable sync provider notifications','Hides cloud provider notifications in File Explorer.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced','ShowSyncProviderNotifications',0);
reg(visual,'Open File Explorer to This PC','Sets File Explorer to open This PC by default.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced','LaunchTo',1);
reg(visual,'Hide recent files in File Explorer home','Disables recent files section in Explorer Home.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer','ShowRecent',0);
reg(visual,'Hide frequent folders in File Explorer home','Disables frequent folders section in Explorer Home.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer','ShowFrequent',0);
reg(visual,'Disable toast banner notifications','Disables toast notifications for the current user.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\PushNotifications','ToastEnabled',0);
service('Disable Print Spooler at startup','Do this only if you do not print or use print-to-PDF workflows that require the service.','Spooler',visual);
service('Disable Fax service at startup','Stops the Fax service from starting automatically. Fax workflows may be unavailable.','Fax',visual);
service('Disable Remote Desktop Configuration service at startup','Prevents the service from starting automatically; remote desktop features may need it.','SessionEnv',visual);
service('Disable Maps Manager at startup','Stops offline maps management from starting automatically.','MapsBroker',visual);
service('Disable Retail Demo service at startup','Stops retail demo functionality from starting automatically.','RetailDemo',visual);
service('Disable Bluetooth service at startup','Do this only if you do not use Bluetooth devices.','bthserv',visual);
service('Disable Smart Card service at startup','Do this only if you do not use smart cards or certificate-based sign-in.','SCardSvr',privacy);
service('Disable media sharing service at startup','Stops Windows Media Player network sharing from starting automatically.','WMPNetworkSvc',visual);
service('Disable Internet Connection Sharing at startup','Stops Internet Connection Sharing from starting automatically.','SharedAccess',network);
service('Disable Mobile Hotspot service at startup','Do this only if you do not use Windows Mobile Hotspot.','icssvc',network);
service('Disable Wallet service at startup','Stops the Wallet service from starting automatically.','WalletService',privacy);
service('Disable Location service at startup','Stops the Geolocation service from starting automatically; location features may stop working.','lfsvc',privacy);
service('Disable Phone service at startup','Stops Phone service from starting automatically.','PhoneSvc',visual);
reg(visual,'Disable background apps policy','Prevents Microsoft Store apps from running in the background. Notifications and background sync for affected apps may stop.','HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\AppPrivacy','LetAppsRunInBackground',2);
service('Disable Connected Devices Platform service at startup','Stops Connected Devices Platform from starting automatically. Nearby sharing and cross-device features may be affected.','CDPSvc',visual);
review(visual,['Disable Search indexing on gaming drives']);

reg(memory,'Disable SysMain service startup','Sets SysMain startup to manual only; services are not stopped by this toggle. Restart may be required.','HKLM\\SYSTEM\\CurrentControlSet\\Services\\SysMain','Start',3,'DWord',true);
reg(memory,'Enable NTFS last access updates off','Disables last-access timestamp updates; some backup/indexing tools may depend on timestamps.','HKLM\\SYSTEM\\CurrentControlSet\\Control\\FileSystem','NtfsDisableLastAccessUpdate',1);
reg(memory,'Set large system cache off','Sets workstation cache preference. Reboot required; server workloads may need the opposite setting.','HKLM\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Memory Management','LargeSystemCache',0,'DWord',true);
reg(memory,'Set foreground app responsiveness','Sets Win32PrioritySeparation to a standard interactive preference.','HKLM\\SYSTEM\\CurrentControlSet\\Control\\PriorityControl','Win32PrioritySeparation',38);
reg(memory,'Disable delivery optimization peer sharing','Limits update delivery to local network peers.','HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\DeliveryOptimization','DODownloadMode',0);
reg(memory,'Disable thumbnail cache on network folders','Avoids storing thumbnail cache data for network folders.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Policies\\Explorer','DisableThumbnailsOnNetworkFolders',1);
reg(memory,'Disable thumbnail cache deletion cleanup','Keeps thumbnail cache across cleanup cycles.','HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced','DisableThumbnailCache',0);
service('Set Windows Search to manual','Leaves Windows Search available on demand instead of starting automatically. Indexing and search results may be delayed.','WSearch',memory);
hibernate('Disable hibernation','Turns off hibernation, hybrid sleep, and Fast Startup while enabled. Saves the prior hibernation state for restore; use only if you accept losing those features.');
review(memory,['Disable Prefetch','Set NTFS memory usage high','Clear standby list','Disable memory compression','Disable pagefile','Optimize TRIM schedule','Disable reserved storage']);

export const catalog = rows;
