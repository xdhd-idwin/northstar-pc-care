param([Parameter(Mandatory=$true)][string]$PayloadPath)
$ErrorActionPreference = 'Stop'

function Write-Result($value) {
  $json = $value | ConvertTo-Json -Depth 8 -Compress
  [System.IO.File]::WriteAllText([string]$req.OutputPath, $json, [System.Text.UTF8Encoding]::new($false))
}
function Test-Admin {
  $identity=[Security.Principal.WindowsIdentity]::GetCurrent()
  return ([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}
function Get-RegPath([string]$key) {
  if($key -notmatch '^(HKCU|HKLM)\\[A-Za-z0-9 _\\.-]+(?:\\\{[0-9A-Fa-f-]{36}\})?$') { throw 'Registry path is outside the supported hives.' }
  return $key -replace '^HKCU','Registry::HKEY_CURRENT_USER' -replace '^HKLM','Registry::HKEY_LOCAL_MACHINE'
}
function Set-RegValue([string]$path,[string]$name,$value,[string]$kind) {
  $hivePath=$path -replace '^Registry::HKEY_CURRENT_USER\\','' -replace '^Registry::HKEY_LOCAL_MACHINE\\',''
  if($path.StartsWith('Registry::HKEY_CURRENT_USER\')) { $hive=[Microsoft.Win32.Registry]::CurrentUser }
  elseif($path.StartsWith('Registry::HKEY_LOCAL_MACHINE\')) { $hive=[Microsoft.Win32.Registry]::LocalMachine }
  else { throw 'Unsupported registry hive.' }
  $rk=$hive.CreateSubKey($hivePath)
  try {
    $vk=[Enum]::Parse([Microsoft.Win32.RegistryValueKind],$kind)
    if($vk -eq [Microsoft.Win32.RegistryValueKind]::DWord) { $value=[int]([long]$value -band 0xffffffffL) }
    elseif($vk -eq [Microsoft.Win32.RegistryValueKind]::QWord) { $value=[long]$value }
    elseif($vk -eq [Microsoft.Win32.RegistryValueKind]::String -or $vk -eq [Microsoft.Win32.RegistryValueKind]::ExpandString) { $value=[string]$value }
    elseif($vk -eq [Microsoft.Win32.RegistryValueKind]::Binary) { $value=[byte[]]@($value|ForEach-Object{[byte]$_}) }
    elseif($vk -eq [Microsoft.Win32.RegistryValueKind]::MultiString) { $value=[string[]]@($value|ForEach-Object{[string]$_}) }
    $rk.SetValue($name,$value,$vk)
  } finally { $rk.Dispose() }
}
function Remove-CreatedEmptyKey($backup) {
  if($backup.keyExisted){return}
  $path=Get-RegPath ([string]$backup.key)
  if(!$path.StartsWith('Registry::HKEY_CURRENT_USER\') -and !$path.StartsWith('Registry::HKEY_LOCAL_MACHINE\')){return}
  $sub=$path -replace '^Registry::HKEY_CURRENT_USER\\','' -replace '^Registry::HKEY_LOCAL_MACHINE\\',''
  try { if($path.StartsWith('Registry::HKEY_CURRENT_USER\')){[Microsoft.Win32.Registry]::CurrentUser.DeleteSubKey($sub,$false)}else{[Microsoft.Win32.Registry]::LocalMachine.DeleteSubKey($sub,$false)} } catch {}
}
function Read-Backup($file) {
  if(Test-Path -LiteralPath $file) { return Get-Content -LiteralPath $file -Raw | ConvertFrom-Json }
  return $null
}
function Invoke-PowerCfg([string[]]$Arguments) {
  $output=@(& powercfg.exe @Arguments 2>&1)
  if($LASTEXITCODE -ne 0){throw ('powercfg failed: '+($output -join ' '))}
  return $output
}
function Get-ActivePowerScheme {
  $output=Invoke-PowerCfg @('/getactivescheme');$match=[regex]::Match(($output -join ' '),'(?i)\b[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}\b')
  if(!$match.Success){throw 'Windows did not return the active power plan identifier.'};return $match.Value.ToLowerInvariant()
}
function Test-PowerScheme([string]$scheme) {
  $output=Invoke-PowerCfg @('/list');return (($output -join ' ') -match ('(?i)\b'+[regex]::Escape($scheme)+'\b'))
}
function Set-ActivePowerScheme([string]$scheme) { Invoke-PowerCfg @('/setactive',$scheme)|Out-Null }
function Remove-PowerScheme([string]$scheme) { if(Test-PowerScheme $scheme){Invoke-PowerCfg @('/delete',$scheme)|Out-Null} }
function Get-PowerSettingDefinition([string]$name) {
  $processor='54533251-82be-4824-96c1-47b60b740d00'
  switch($name){
    'minProcessor' { return @{subgroup=$processor;setting='893dee8e-2bef-41e0-89c6-b55d0929964c'} }
    'maxProcessor' { return @{subgroup=$processor;setting='bc5038f7-23e0-4960-96da-33abaf5935ec'} }
    'minUnparkedCores' { return @{subgroup=$processor;setting='0cc5b647-c1df-4637-891a-dec35c318583'} }
    'maxUnparkedCores' { return @{subgroup=$processor;setting='ea062031-0e34-4ff1-9b6d-eb1059334028'} }
    'usbSelectiveSuspend' { return @{subgroup='2a737441-1930-4402-8d77-b2bebba308a3';setting='48e6b7a6-50f5-4782-a5d4-53bb8f07e226'} }
    default { throw 'Unsupported processor power setting.' }
  }
}
function Get-PowerSettingIndex([string]$scheme,[string]$subgroup,[string]$setting,[ValidateSet('AC','DC')][string]$mode) {
  $plan=Get-WmiObject -Namespace 'root\cimv2\power' -Class Win32_PowerPlan -ErrorAction Stop|Where-Object{[string]$_.InstanceID -match ('(?i)\{'+[regex]::Escape($scheme)+'\}$')}|Select-Object -First 1
  if(!$plan){throw ('Could not read power plan '+$scheme+'.')}
  foreach($row in @($plan.GetRelated('Win32_PowerSettingDataIndex'))){
    if([string]$row.InstanceID -notmatch ('(?i)\\'+$mode+'\\\{[0-9a-f-]{36}\}$')){continue}
    foreach($relatedSetting in @($row.GetRelated('Win32_PowerSetting'))){if([string]$relatedSetting.InstanceID -match ('(?i)\{'+[regex]::Escape($setting)+'\}$')){return [uint32]$row.SettingIndexValue}}
  }
  throw ('Could not read the '+$mode+' value for power setting '+$setting+'.')
}
function Set-PowerSettingIndices([string]$scheme,[string]$subgroup,[string]$setting,[uint32]$ac,[uint32]$dc,[bool]$activate=$true) {
  Invoke-PowerCfg @('/setacvalueindex',$scheme,$subgroup,$setting,[string]$ac)|Out-Null
  Invoke-PowerCfg @('/setdcvalueindex',$scheme,$subgroup,$setting,[string]$dc)|Out-Null
  if($activate){Invoke-PowerCfg @('/setactive',$scheme)|Out-Null}
}
function Get-HibernationEnabled {
  $path=Get-RegPath 'HKLM\SYSTEM\CurrentControlSet\Control\Power'
  if(Test-Path -LiteralPath $path){$key=Get-Item -LiteralPath $path;if($key.GetValueNames() -contains 'HibernateEnabled'){return ([int]$key.GetValue('HibernateEnabled') -ne 0)}}
  return (Test-Path -LiteralPath (Join-Path $env:SystemDrive 'hiberfil.sys'))
}
function Set-HibernationEnabled([bool]$enabled) { if($enabled){Invoke-PowerCfg @('/hibernate','on')|Out-Null}else{Invoke-PowerCfg @('/hibernate','off')|Out-Null} }
function Get-SnapshotList {
  if(!(Test-Path -LiteralPath $req.DataDir)) { return @() }
  $list=@()
  $backupRoot=Join-Path $req.DataDir 'backups'
  Get-ChildItem -LiteralPath $backupRoot -Filter '*.json' -File -ErrorAction SilentlyContinue | ForEach-Object {
    try { $b=Get-Content -LiteralPath $_.FullName -Raw | ConvertFrom-Json; $list += [pscustomobject]@{id=$b.id;enabled=[bool]$b.enabled;restorable=$true} } catch {}
  }
  $serviceRoot=Join-Path $req.DataDir 'services'
  Get-ChildItem -LiteralPath $serviceRoot -Filter '*.json' -File -ErrorAction SilentlyContinue | ForEach-Object {
    try { $b=Get-Content -LiteralPath $_.FullName -Raw | ConvertFrom-Json; $list += [pscustomobject]@{id=$b.id;enabled=[bool]$b.enabled;restorable=$true} } catch {}
  }
  foreach($folder in @('powerplans','powersettings','nagle','hibernation','compatibilitytasks')){$powerRoot=Join-Path $req.DataDir $folder;Get-ChildItem -LiteralPath $powerRoot -Filter '*.json' -File -ErrorAction SilentlyContinue|ForEach-Object{try{$b=Get-Content -LiteralPath $_.FullName -Raw|ConvertFrom-Json;$list += [pscustomobject]@{id=$b.id;enabled=[bool]$b.enabled;restorable=$true}}catch{}}}
  return $list
}
function Restore-ServiceBackup($backup) {
  if(!$backup){return}
  $startupType=switch([string]$backup.startMode){'Auto'{'Automatic'} 'Manual'{'Manual'} 'Disabled'{'Disabled'} default{throw ('Unknown saved service start mode: '+$backup.startMode)}}
  Set-Service -Name ([string]$backup.serviceName) -StartupType $startupType -ErrorAction Stop
  $serviceKey='HKLM\SYSTEM\CurrentControlSet\Services\'+[string]$backup.serviceName
  $servicePath=Get-RegPath $serviceKey
  if([bool]$backup.delayedExists){Set-RegValue $servicePath 'DelayedAutoStart' $backup.delayedAutoStart ([string]$backup.delayedKind)}elseif(Test-Path -LiteralPath $servicePath){Remove-ItemProperty -LiteralPath $servicePath -Name 'DelayedAutoStart' -ErrorAction SilentlyContinue}
}
function Get-StartupEntries {
  $result=@();$targets=@(@{key='HKCU\Software\Microsoft\Windows\CurrentVersion\Run';scope='Current user'},@{key='HKLM\Software\Microsoft\Windows\CurrentVersion\Run';scope='All users'})
  foreach($target in $targets) {
    try { $rp=Get-RegPath $target.key;if(Test-Path -LiteralPath $rp){$rk=Get-Item -LiteralPath $rp;foreach($n in $rk.GetValueNames()){$cmd=[string]$rk.GetValue($n,$null,[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames);$sha=[Security.Cryptography.SHA256]::Create();try{$hash=$sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($target.key+'|'+$n));$id=[BitConverter]::ToString($hash).Replace('-','').ToLowerInvariant()}finally{$sha.Dispose()};$result += [pscustomobject]@{id=$id;name=$n;command=$cmd;scope=$target.scope;enabled=$true;source='Registry Run'}}} } catch {}
  }
  $backupRoot=Join-Path $req.DataDir 'startup';if(Test-Path -LiteralPath $backupRoot){Get-ChildItem -LiteralPath $backupRoot -Filter '*.json' -File|ForEach-Object{try{$b=Get-Content -LiteralPath $_.FullName -Raw|ConvertFrom-Json;$result += [pscustomobject]@{id=$b.id;name=$b.name;command=$b.value;scope=$b.scope;enabled=$false;source='Registry Run'}}catch{}}}
  return $result|Sort-Object scope,name -Unique
}
function Get-ScheduledStartupEntries {
  $result=@()
  try {$tasks=Get-ScheduledTask -ErrorAction Stop} catch {return @()}
  foreach($task in $tasks) {
    $taskPath=[string]$task.TaskPath;$taskName=[string]$task.TaskName
    if($taskPath -match '^\\Microsoft\\Windows\\'){continue}
    $triggered=$false;foreach($trigger in @($task.Triggers)){if($trigger.CimClass.CimClassName -match 'Logon|Boot'){$triggered=$true;break}}
    if(!$triggered){continue}
    $sha=[Security.Cryptography.SHA256]::Create();try{$hash=$sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($taskPath+'|'+$taskName));$id=[BitConverter]::ToString($hash).Replace('-','').ToLowerInvariant()}finally{$sha.Dispose()}
    $command=(@($task.Actions|ForEach-Object{(@($_.Execute,$_.Arguments)|Where-Object{$_}) -join ' '}) -join '; ')
    $result += [pscustomobject]@{id=$id;name=$taskName;command=$command;scope=$taskPath;enabled=([string]$task.State -ne 'Disabled');source='Scheduled task'}
  }
  $backupRoot=Join-Path $req.DataDir 'scheduled';if(Test-Path -LiteralPath $backupRoot){Get-ChildItem -LiteralPath $backupRoot -Filter '*.json' -File|ForEach-Object{try{$b=Get-Content -LiteralPath $_.FullName -Raw|ConvertFrom-Json;$result += [pscustomobject]@{id=$b.id;name=$b.taskName;command=$b.command;scope=$b.taskPath;enabled=$false;source='Scheduled task'}}catch{}}}
  return $result|Sort-Object scope,name -Unique
}
function Get-CompatibilityTelemetryTasks {
  $targets=@(
    @{path='\Microsoft\Windows\Application Experience\';name='Microsoft Compatibility Appraiser'},
    @{path='\Microsoft\Windows\Application Experience\';name='ProgramDataUpdater'},
    @{path='\Microsoft\Windows\Customer Experience Improvement Program\';name='Consolidator'},
    @{path='\Microsoft\Windows\Customer Experience Improvement Program\';name='UsbCeip'},
    @{path='\Microsoft\Windows\Customer Experience Improvement Program\';name='KernelCeipTask'}
  );$found=@()
  foreach($target in $targets){try{$task=Get-ScheduledTask -TaskPath $target.path -TaskName $target.name -ErrorAction Stop;$found+=@{taskPath=[string]$target.path;taskName=[string]$target.name;enabled=([string]$task.State -ne 'Disabled')}}catch{}}
  return $found
}
function Convert-NetworkInterfaceIndex($value) {
  $parsed=0
  if($null -eq $value -or ![int]::TryParse([string]$value,[ref]$parsed) -or $parsed -lt 1 -or $parsed -gt 65535) { throw 'The network adapter ID is invalid. Refresh the adapter list and try again.' }
  return $parsed
}
function Set-DnsFamily($interfaceIndex,[string]$family,[string[]]$addresses) {
  $ix=Convert-NetworkInterfaceIndex $interfaceIndex
  if(@($addresses).Count -gt 0){$parameters=@{InterfaceIndex=$ix;AddressFamily=$family;ServerAddresses=@($addresses);ErrorAction='Stop'};Set-DnsClientServerAddress @parameters}
  else {$parameters=@{InterfaceIndex=$ix;AddressFamily=$family;ResetServerAddresses=$true;ErrorAction='Stop'};Set-DnsClientServerAddress @parameters}
}
function Restore-DnsBackup($backup) {
  $ix=Convert-NetworkInterfaceIndex $backup.interfaceIndex
  Set-DnsFamily $ix 'IPv4' @($backup.ipv4)
  Set-DnsFamily $ix 'IPv6' @($backup.ipv6)
}
function Get-CleanupRoots {
  $roots=@($env:TEMP,(Join-Path $env:WINDIR 'Temp'),(Join-Path $env:LOCALAPPDATA 'D3DSCache'),(Join-Path $env:LOCALAPPDATA 'NVIDIA\DXCache'),(Join-Path $env:LOCALAPPDATA 'AMD\DxCache'),(Join-Path $env:PROGRAMDATA 'NVIDIA Corporation\NV_Cache'))|Where-Object{$_}
  $browserRunning=@(Get-Process -Name chrome,msedge,firefox -ErrorAction SilentlyContinue).Count -gt 0
  if(!$browserRunning) {
    foreach($browserRoot in @((Join-Path $env:LOCALAPPDATA 'Google\Chrome\User Data'),(Join-Path $env:LOCALAPPDATA 'Microsoft\Edge\User Data'))){
      if(Test-Path -LiteralPath $browserRoot){foreach($profile in (Get-ChildItem -LiteralPath $browserRoot -Directory -Force -ErrorAction SilentlyContinue)){foreach($cacheName in @('Cache','Code Cache','GPUCache')){$roots += Join-Path $profile.FullName $cacheName}}}
    }
    $firefoxRoot=Join-Path $env:LOCALAPPDATA 'Mozilla\Firefox\Profiles';if(Test-Path -LiteralPath $firefoxRoot){foreach($profile in (Get-ChildItem -LiteralPath $firefoxRoot -Directory -Force -ErrorAction SilentlyContinue)){$roots += Join-Path $profile.FullName 'cache2'}}
  }
  return @($roots|Where-Object{$_ -and (Test-Path -LiteralPath $_ -PathType Container)}|Select-Object -Unique)
}
function Ensure-RestorePoint {
  if($req.PSObject.Properties.Name -contains 'CreateRestorePoint' -and !$req.CreateRestorePoint) { return 'Windows restore point skipped by your choice; the app saved an individual settings backup.' }
  try { Checkpoint-Computer -Description 'Northstar PC Care change' -RestorePointType 'MODIFY_SETTINGS' -ErrorAction Stop; return $null }
  catch { return ('System Restore point was unavailable: ' + $_.Exception.Message) }
}
function Add-Log([string]$message) {
  New-Item -ItemType Directory -Path $req.DataDir -Force | Out-Null
  $log=Join-Path $req.DataDir 'operations.json'
  $items=@(); if(Test-Path -LiteralPath $log) { try{$items=@(Get-Content -LiteralPath $log -Raw|ConvertFrom-Json)}catch{} }
  $items += [pscustomobject]@{time=(Get-Date).ToUniversalTime().ToString('o');message=$message}
  [System.IO.File]::WriteAllText($log,($items|ConvertTo-Json -Depth 4),[System.Text.UTF8Encoding]::new($false))
}
try {
  $req=Get-Content -LiteralPath $PayloadPath -Raw | ConvertFrom-Json
  if($req.Action -in @('Tweak','TweakBatch','RevertAll','CleanRun','StartupSet','ScheduledStartupSet','DnsSet','DnsRestore','DnsFlush') -and !(Test-Admin)) { Write-Result @{ok=$false;code='ELEVATION_REQUIRED';message='Administrator permission is required.'}; exit 0 }
  switch($req.Action) {
    'Metrics' {
      $cpu=$null;$memory=$null;$disk=$null;$gpu=$null;$ping=$null;$total=$null;$uptime=$null
      try { $os=Get-CimInstance Win32_OperatingSystem; $total=[math]::Round($os.TotalVisibleMemorySize/1MB,1); $memory=[math]::Round((1-($os.FreePhysicalMemory/$os.TotalVisibleMemorySize))*100,1); $uptime=[math]::Round(((Get-Date)-$os.LastBootUpTime).TotalHours,1) } catch {}
      try { $cpu=[math]::Round((Get-CimInstance Win32_Processor|Measure-Object -Property LoadPercentage -Average).Average,0) } catch {}
      try { $disk=[math]::Round((Get-Counter '\LogicalDisk(_Total)\% Disk Time' -ErrorAction Stop).CounterSamples[0].CookedValue,0) } catch {}
      try { $gs=(Get-Counter '\GPU Engine(*)\Utilization Percentage' -ErrorAction Stop).CounterSamples|Measure-Object -Property CookedValue -Sum; $gpu=[math]::Min(100,[math]::Max(0,[math]::Round($gs.Sum,0))) } catch {}
      try { $pingClient=New-Object System.Net.NetworkInformation.Ping;$reply=$pingClient.Send('1.1.1.1',1500);if($reply.Status -eq [System.Net.NetworkInformation.IPStatus]::Success){$ping=[math]::Round($reply.RoundtripTime,0)} } catch {}
      Write-Result @{ok=$true;data=@{cpu=$cpu;memory=$memory;disk=$disk;gpu=$gpu;ping=$ping;totalMemoryGb=$total;uptimeHours=$uptime}}; break
    }
    'Snapshots' { Write-Result @{ok=$true;data=@(Get-SnapshotList)}; break }
    'StartupScan' { Write-Result @{ok=$true;data=@(Get-StartupEntries)}; break }
    'StartupSet' {
      $id=[string]$req.Id;if($id -notmatch '^[a-f0-9]{64}$'){throw 'Invalid startup item identifier.'}
      $items=@(Get-StartupEntries);$entry=$items|Where-Object{$_.id -eq $id}|Select-Object -First 1;if(!$entry){throw 'Startup item is no longer available.'}
      $key=if($entry.scope -eq 'Current user'){'HKCU\Software\Microsoft\Windows\CurrentVersion\Run'}else{'HKLM\Software\Microsoft\Windows\CurrentVersion\Run'};$rp=Get-RegPath $key;$dir=Join-Path $req.DataDir 'startup';New-Item -ItemType Directory -Path $dir -Force|Out-Null;$file=Join-Path $dir ($id+'.json')
      if([bool]$req.Enable){$b=Read-Backup $file;if(!$b){throw 'Saved startup command was not found.'};Set-RegValue (Get-RegPath $b.key) ([string]$b.name) $b.value ([string]$b.kind);Remove-Item -LiteralPath $file -Force}
      else {if(!$entry.enabled){Write-Result @{ok=$true;message='Startup item is already disabled.'};break};$rk=Get-Item -LiteralPath $rp;$kind=[string]$rk.GetValueKind($entry.name);$value=$rk.GetValue($entry.name,$null,[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames);@{id=$id;key=$key;name=$entry.name;value=$value;kind=$kind;scope=$entry.scope}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $file -Encoding UTF8;Remove-ItemProperty -LiteralPath $rp -Name $entry.name -ErrorAction Stop}
      Add-Log ('Updated startup item '+$id);Write-Result @{ok=$true;message='Startup item updated. Restart or sign out to see the change.'};break
    }
    'ScheduledStartupScan' { Write-Result @{ok=$true;data=@(Get-ScheduledStartupEntries)};break }
    'ScheduledStartupSet' {
      $id=[string]$req.Id;if($id -notmatch '^[a-f0-9]{64}$'){throw 'Invalid scheduled task identifier.'}
      $entry=@(Get-ScheduledStartupEntries)|Where-Object{$_.id -eq $id}|Select-Object -First 1;if(!$entry){throw 'Scheduled startup task is no longer available.'}
      $taskPath=[string]$entry.scope;$taskName=[string]$entry.name;if($taskPath -match '^\\Microsoft\\Windows\\'){throw 'Built-in Windows scheduled tasks are read-only in this manager.'}
      $backupRoot=Join-Path $req.DataDir 'scheduled';New-Item -ItemType Directory -Path $backupRoot -Force|Out-Null;$backup=Join-Path $backupRoot ($id+'.json')
      if([bool]$req.Enable){Enable-ScheduledTask -TaskName $taskName -TaskPath $taskPath -ErrorAction Stop|Out-Null;if(Test-Path -LiteralPath $backup){Remove-Item -LiteralPath $backup -Force}}
      else {if(!$entry.enabled){Write-Result @{ok=$true;message='Scheduled task is already disabled.'};break};$restoreWarning=Ensure-RestorePoint;@{id=$id;taskName=$taskName;taskPath=$taskPath;command=$entry.command;enabled=$true}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $backup -Encoding UTF8;Disable-ScheduledTask -TaskName $taskName -TaskPath $taskPath -ErrorAction Stop|Out-Null}
      Add-Log ('Updated scheduled startup task '+$id);$msg='Scheduled startup task updated.';if($restoreWarning){$msg+=' '+$restoreWarning};Write-Result @{ok=$true;message=$msg};break
    }
    'DnsScan' {
      $adapters=@();foreach($net in (Get-NetAdapter -ErrorAction SilentlyContinue|Where-Object{$_.Status -eq 'Up'})){
        $ix=Convert-NetworkInterfaceIndex $net.ifIndex
        $v4=@(Get-DnsClientServerAddress -InterfaceIndex $ix -AddressFamily IPv4 -ErrorAction SilentlyContinue|ForEach-Object{$_.ServerAddresses})
        $v6=@(Get-DnsClientServerAddress -InterfaceIndex $ix -AddressFamily IPv6 -ErrorAction SilentlyContinue|ForEach-Object{$_.ServerAddresses})
        $adapters += [pscustomobject]@{index=$ix;name=[string]$net.Name;ipv4=$v4;ipv6=$v6}
      }
      Write-Result @{ok=$true;data=@($adapters)};break
    }
    'DnsSet' {
      $ix=Convert-NetworkInterfaceIndex $req.InterfaceIndex;$profile=[string]$req.Provider;if($profile -notin @('cloudflare','google')){throw 'Invalid DNS provider.'}
      $adapter=Get-NetAdapter -InterfaceIndex $ix -ErrorAction Stop;if($adapter.Status -ne 'Up'){throw 'The selected network adapter is not connected.'}
      $dnsRoot=Join-Path $req.DataDir 'dns';New-Item -ItemType Directory -Path $dnsRoot -Force|Out-Null;$backup=Join-Path $dnsRoot ($ix+'.json')
      if(!(Test-Path -LiteralPath $backup)){
        $v4=@(Get-DnsClientServerAddress -InterfaceIndex $ix -AddressFamily IPv4 -ErrorAction Stop|ForEach-Object{$_.ServerAddresses});$v6=@(Get-DnsClientServerAddress -InterfaceIndex $ix -AddressFamily IPv6 -ErrorAction SilentlyContinue|ForEach-Object{$_.ServerAddresses})
        @{interfaceIndex=$ix;adapterName=[string]$adapter.Name;ipv4=$v4;ipv6=$v6;enabled=$false}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $backup -Encoding UTF8
      }
      $restoreWarning=Ensure-RestorePoint
      if($profile -eq 'cloudflare'){$v4=@('1.1.1.1','1.0.0.1');$v6=@('2606:4700:4700::1111','2606:4700:4700::1001')}else{$v4=@('8.8.8.8','8.8.4.4');$v6=@('2001:4860:4860::8888','2001:4860:4860::8844')}
      try {Set-DnsFamily $ix 'IPv4' $v4;Set-DnsFamily $ix 'IPv6' $v6}
      catch {$b=Read-Backup $backup;Restore-DnsBackup $b;throw}
      $b=Read-Backup $backup;$b.enabled=$true;$b|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $backup -Encoding UTF8;Add-Log ('Set DNS profile '+$profile+' on interface '+$ix)
      $msg=('Applied '+$profile+' DNS to '+$adapter.Name+'.');if($restoreWarning){$msg+=' '+$restoreWarning};Write-Result @{ok=$true;message=$msg};break
    }
    'DnsRestore' {
      $ix=Convert-NetworkInterfaceIndex $req.InterfaceIndex;$backup=Join-Path (Join-Path $req.DataDir 'dns') ($ix+'.json');$b=Read-Backup $backup
      if(!$b){Write-Result @{ok=$true;message='No saved DNS settings are available for this adapter.'};break}
      Restore-DnsBackup $b;Remove-Item -LiteralPath $backup -Force;Add-Log ('Restored DNS on interface '+$ix);Write-Result @{ok=$true;message=('Original DNS settings restored on '+$b.adapterName+'.')};break
    }
    'DnsFlush' { Clear-DnsClientCache -ErrorAction Stop;Write-Result @{ok=$true;message='Windows DNS resolver cache flushed.'};break }
    'TweakBatch' {
      $items=@($req.Items);if($items.Count -lt 1 -or $items.Count -gt 20){throw 'Invalid optimization profile.'}
      $restoreWarning=Ensure-RestorePoint;$applied=0
      foreach($entry in $items){
        try {
          $id=[string]$entry.id;$t=$entry.tweak
          if($id -notmatch '^safe-[0-9]{1,3}$' -or !$t -or [string]$t.kind -notin @('DWord','String')){throw 'Invalid optimization item.'}
          $regPath=Get-RegPath ([string]$t.key);$backupDir=Join-Path $req.DataDir 'backups';New-Item -ItemType Directory -Path $backupDir -Force|Out-Null;$backup=Join-Path $backupDir ($id+'.json')
          if(!(Test-Path -LiteralPath $backup)){
            $exists=Test-Path -LiteralPath $regPath;$present=$false;$old=$null;$oldKind=$null
            if($exists){$keyObj=Get-Item -LiteralPath $regPath;if($keyObj.GetValueNames() -contains [string]$t.name){$present=$true;$old=$keyObj.GetValue([string]$t.name,$null,[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames);$oldKind=[string]$keyObj.GetValueKind([string]$t.name)}}
            @{id=$id;key=[string]$t.key;name=[string]$t.name;keyExisted=$exists;valueExisted=$present;value=$old;kind=$oldKind;enabled=$false}|ConvertTo-Json -Depth 6|Set-Content -LiteralPath $backup -Encoding UTF8
          }
          New-Item -Path $regPath -Force|Out-Null;Set-RegValue $regPath ([string]$t.name) $t.value ([string]$t.kind)
          $b=Read-Backup $backup;$b.enabled=$true;$b|ConvertTo-Json -Depth 6|Set-Content -LiteralPath $backup -Encoding UTF8;Add-Log ('Applied '+$id);$applied++
        } catch { Write-Result @{ok=$false;message=('Profile applied '+$applied+' of '+$items.Count+' settings before this error: '+$_.Exception.Message);code='PARTIAL_APPLY'};return }
      }
      $msg=('Applied '+$applied+' reviewed settings.');if($restoreWarning){$msg+=' '+$restoreWarning};Write-Result @{ok=$true;message=$msg};break
    }
    'Tweak' {
      $id=[string]$req.Id; $t=$req.Tweak
      if($id -notmatch '^safe-[0-9]{1,3}$' -or !$t -or [string]$t.kind -notin @('DWord','String','Service','PowerPlan','PowerSetting','NetworkNagle','Hibernate','CompatibilityTasks')) { throw 'Invalid allowlisted tweak payload.' }
      if([string]$t.kind -eq 'Service') {
        $serviceName=[string]$t.serviceName;if($serviceName -notmatch '^[A-Za-z0-9_-]{2,80}$'){throw 'Invalid service name.'}
        $backupDir=Join-Path $req.DataDir 'services';New-Item -ItemType Directory -Path $backupDir -Force|Out-Null;$backup=Join-Path $backupDir ($id+'.json')
        $serviceKey='HKLM\SYSTEM\CurrentControlSet\Services\'+$serviceName;$servicePath=Get-RegPath $serviceKey
        if([bool]$req.Enable) {
          $restoreWarning=Ensure-RestorePoint
          if(!(Test-Path -LiteralPath $backup)) {
            $svc=Get-CimInstance -ClassName Win32_Service -Filter ("Name='"+$serviceName+"'") -ErrorAction Stop;if(!$svc){throw 'This Windows service is not installed.'}
            $delayedExists=$false;$delayedValue=0;$delayedKind='DWord'
            if(Test-Path -LiteralPath $servicePath){$rk=Get-Item -LiteralPath $servicePath;if($rk.GetValueNames() -contains 'DelayedAutoStart'){$delayedExists=$true;$delayedValue=$rk.GetValue('DelayedAutoStart');$delayedKind=[string]$rk.GetValueKind('DelayedAutoStart')}}
            @{id=$id;serviceName=$serviceName;startMode=[string]$svc.StartMode;delayedExists=$delayedExists;delayedAutoStart=$delayedValue;delayedKind=$delayedKind;enabled=$false}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $backup -Encoding UTF8
          }
          Set-Service -Name $serviceName -StartupType Disabled -ErrorAction Stop
          $b=Read-Backup $backup;$b.enabled=$true;$b|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $backup -Encoding UTF8;Add-Log ('Set service startup disabled: '+$serviceName)
          $msg='Service automatic startup disabled. A running service was left running; restart may be required.';if($restoreWarning){$msg+=' '+$restoreWarning};Write-Result @{ok=$true;message=$msg};break
        } else {
          $b=Read-Backup $backup;if(!$b -or !$b.enabled){Write-Result @{ok=$true;message='No service startup change is active.'};break}
          Restore-ServiceBackup $b;$b.enabled=$false;$b|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $backup -Encoding UTF8;Add-Log ('Restored service startup: '+$serviceName);Write-Result @{ok=$true;message='Original service startup mode restored.'};break
        }
      }
      if([string]$t.kind -eq 'PowerPlan') {
        $powerDir=Join-Path $req.DataDir 'powerplans';New-Item -ItemType Directory -Path $powerDir -Force|Out-Null;$powerBackup=Join-Path $powerDir ($id+'.json')
        if([bool]$req.Enable) {
          $restoreWarning=Ensure-RestorePoint;$original=Get-ActivePowerScheme;$ultimate='e9a42b02-d5df-448d-aa00-03f14749eb61';$created=$false;$target=$ultimate
          if(!(Test-PowerScheme $ultimate)){$duplicate=Invoke-PowerCfg @('/duplicatescheme',$ultimate);$matches=[regex]::Matches(($duplicate -join ' '),'(?i)\b[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}\b');if($matches.Count -lt 1){throw 'Windows did not return the new Ultimate Performance plan identifier.'};$target=$matches[$matches.Count-1].Value.ToLowerInvariant();$created=$true}
          @{id=$id;originalScheme=$original;targetScheme=$target;createdByApp=$created;enabled=$false}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $powerBackup -Encoding UTF8
          try {Set-ActivePowerScheme $target} catch {if($created){Remove-PowerScheme $target};throw}
          $b=Read-Backup $powerBackup;$b.enabled=$true;$b|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $powerBackup -Encoding UTF8;Add-Log 'Activated Ultimate Performance power plan'
          $msg='Ultimate Performance power plan activated. Restore this toggle to return to the prior plan.';if($restoreWarning){$msg+=' '+$restoreWarning};Write-Result @{ok=$true;message=$msg};break
        } else {
          $b=Read-Backup $powerBackup;if(!$b -or !$b.enabled){Write-Result @{ok=$true;message='No power plan change is active.'};break}
          if(!(Test-PowerScheme ([string]$b.originalScheme))){throw 'The previously active power plan no longer exists; it was not possible to restore it.'}
          $current=Get-ActivePowerScheme;if($current -eq [string]$b.targetScheme){Set-ActivePowerScheme ([string]$b.originalScheme)};if([bool]$b.createdByApp){Remove-PowerScheme ([string]$b.targetScheme)};$b.enabled=$false;$b|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $powerBackup -Encoding UTF8;Add-Log 'Restored previous power plan';Write-Result @{ok=$true;message='Power plan change reverted; a later user-selected plan was preserved.'};break
        }
      }
      if([string]$t.kind -eq 'PowerSetting') {
        $powerDir=Join-Path $req.DataDir 'powersettings';New-Item -ItemType Directory -Path $powerDir -Force|Out-Null;$powerBackup=Join-Path $powerDir ($id+'.json')
        $definition=Get-PowerSettingDefinition ([string]$t.powerSetting)
        if([bool]$req.Enable) {
          $ac=[uint32]$t.powerAc;$dc=[uint32]$t.powerDc;if($ac -gt 100 -or $dc -gt 100){throw 'Processor power values must be between 0 and 100.'}
          $restoreWarning=Ensure-RestorePoint;$scheme=Get-ActivePowerScheme;$oldAc=Get-PowerSettingIndex $scheme $definition.subgroup $definition.setting 'AC';$oldDc=Get-PowerSettingIndex $scheme $definition.subgroup $definition.setting 'DC'
          @{id=$id;scheme=$scheme;subgroup=$definition.subgroup;setting=$definition.setting;oldAc=$oldAc;oldDc=$oldDc;enabled=$false}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $powerBackup -Encoding UTF8
          try {Set-PowerSettingIndices $scheme $definition.subgroup $definition.setting $ac $dc} catch {try{Set-PowerSettingIndices $scheme $definition.subgroup $definition.setting $oldAc $oldDc}catch{};throw}
          $b=Read-Backup $powerBackup;$b.enabled=$true;$b|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $powerBackup -Encoding UTF8;Add-Log ('Updated power setting '+[string]$t.powerSetting)
          $msg='Processor power setting changed. The prior AC/DC values were saved.';if($restoreWarning){$msg+=' '+$restoreWarning};Write-Result @{ok=$true;message=$msg};break
        } else {
          $b=Read-Backup $powerBackup;if(!$b -or !$b.enabled){Write-Result @{ok=$true;message='No processor power setting change is active.'};break}
          $activate=((Get-ActivePowerScheme) -eq [string]$b.scheme);Set-PowerSettingIndices ([string]$b.scheme) ([string]$b.subgroup) ([string]$b.setting) ([uint32]$b.oldAc) ([uint32]$b.oldDc) $activate;$b.enabled=$false;$b|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $powerBackup -Encoding UTF8;Add-Log 'Restored previous processor power values';Write-Result @{ok=$true;message='Previous processor AC/DC values restored.'};break
        }
      }
      if([string]$t.kind -eq 'NetworkNagle') {
        $nagleDir=Join-Path $req.DataDir 'nagle';New-Item -ItemType Directory -Path $nagleDir -Force|Out-Null;$nagleBackup=Join-Path $nagleDir ($id+'.json');$b=Read-Backup $nagleBackup
        if([bool]$req.Enable) {
          if($b -and $b.enabled){Write-Result @{ok=$true;message='Per-adapter Nagle settings are already active.'};break}
          $restoreWarning=Ensure-RestorePoint;$rootKey='HKLM\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces';$rootPath=Get-RegPath $rootKey;$saved=@()
          foreach($interface in @(Get-ChildItem -LiteralPath $rootPath -ErrorAction SilentlyContinue)){
            $guid=[string]$interface.PSChildName;if($guid -notmatch '^[0-9a-fA-F-]{36}$'){continue};$key=$rootKey+'\'+$guid;$path=Get-RegPath $key;$item=Get-Item -LiteralPath $path -ErrorAction Stop;$values=@()
            foreach($name in @('TcpAckFrequency','TCPNoDelay')){$present=$item.GetValueNames() -contains $name;$value=$null;$kind=$null;if($present){$value=$item.GetValue($name,$null,[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames);$kind=[string]$item.GetValueKind($name)};$values+=@{name=$name;present=$present;value=$value;kind=$kind}}
            $saved+=@{key=$key;values=$values}
          }
          if($saved.Count -eq 0){throw 'No TCP/IP adapter interface settings were found.'}
          @{id=$id;interfaces=$saved;enabled=$false}|ConvertTo-Json -Depth 8|Set-Content -LiteralPath $nagleBackup -Encoding UTF8
          try {foreach($entry in $saved){$path=Get-RegPath ([string]$entry.key);Set-RegValue $path 'TcpAckFrequency' 1 'DWord';Set-RegValue $path 'TCPNoDelay' 1 'DWord'}}catch{foreach($entry in $saved){$path=Get-RegPath ([string]$entry.key);foreach($value in @($entry.values)){if([bool]$value.present){Set-RegValue $path ([string]$value.name) $value.value ([string]$value.kind)}elseif(Test-Path -LiteralPath $path){Remove-ItemProperty -LiteralPath $path -Name ([string]$value.name) -ErrorAction SilentlyContinue}}};throw}
          $b=Read-Backup $nagleBackup;$b.enabled=$true;$b|ConvertTo-Json -Depth 8|Set-Content -LiteralPath $nagleBackup -Encoding UTF8;Add-Log 'Disabled Nagle on installed TCP/IP adapters'
          $msg=('Disabled Nagle on '+$saved.Count+' adapter interface(s).');if($restoreWarning){$msg+=' '+$restoreWarning};Write-Result @{ok=$true;message=$msg};break
        } else {
          if(!$b -or !$b.enabled){Write-Result @{ok=$true;message='No per-adapter Nagle change is active.'};break}
          foreach($entry in @($b.interfaces)){$path=Get-RegPath ([string]$entry.key);foreach($value in @($entry.values)){if([bool]$value.present){Set-RegValue $path ([string]$value.name) $value.value ([string]$value.kind)}elseif(Test-Path -LiteralPath $path){Remove-ItemProperty -LiteralPath $path -Name ([string]$value.name) -ErrorAction SilentlyContinue}}}
          $b.enabled=$false;$b|ConvertTo-Json -Depth 8|Set-Content -LiteralPath $nagleBackup -Encoding UTF8;Add-Log 'Restored per-adapter Nagle settings';Write-Result @{ok=$true;message='Original per-adapter Nagle values restored.'};break
        }
      }
      if([string]$t.kind -eq 'Hibernate') {
        $hibernateDir=Join-Path $req.DataDir 'hibernation';New-Item -ItemType Directory -Path $hibernateDir -Force|Out-Null;$hibernateBackup=Join-Path $hibernateDir ($id+'.json');$b=Read-Backup $hibernateBackup
        if([bool]$req.Enable) {
          if($b -and $b.enabled){Write-Result @{ok=$true;message='Hibernation is already disabled by this app.'};break}
          $restoreWarning=Ensure-RestorePoint;$wasEnabled=Get-HibernationEnabled;@{id=$id;wasEnabled=$wasEnabled;enabled=$false}|ConvertTo-Json -Depth 4|Set-Content -LiteralPath $hibernateBackup -Encoding UTF8
          Set-HibernationEnabled $false;$b=Read-Backup $hibernateBackup;$b.enabled=$true;$b|ConvertTo-Json -Depth 4|Set-Content -LiteralPath $hibernateBackup -Encoding UTF8;Add-Log 'Disabled hibernation'
          $msg='Hibernation disabled. Hybrid sleep and Fast Startup are also unavailable.';if($restoreWarning){$msg+=' '+$restoreWarning};Write-Result @{ok=$true;message=$msg};break
        } else {
          if(!$b -or !$b.enabled){Write-Result @{ok=$true;message='No hibernation change is active.'};break}
          Set-HibernationEnabled ([bool]$b.wasEnabled);$b.enabled=$false;$b|ConvertTo-Json -Depth 4|Set-Content -LiteralPath $hibernateBackup -Encoding UTF8;Add-Log 'Restored hibernation state';Write-Result @{ok=$true;message='Original hibernation on/off state restored.'};break
        }
      }
      if([string]$t.kind -eq 'CompatibilityTasks') {
        $taskDir=Join-Path $req.DataDir 'compatibilitytasks';New-Item -ItemType Directory -Path $taskDir -Force|Out-Null;$taskBackup=Join-Path $taskDir ($id+'.json');$b=Read-Backup $taskBackup
        if([bool]$req.Enable) {
          if($b -and $b.enabled){Write-Result @{ok=$true;message='Compatibility telemetry tasks are already disabled by this app.'};break}
          $restoreWarning=Ensure-RestorePoint;$tasks=@(Get-CompatibilityTelemetryTasks);if($tasks.Count -eq 0){throw 'No supported compatibility telemetry tasks were found on this Windows build.'}
          @{id=$id;tasks=$tasks;enabled=$false}|ConvertTo-Json -Depth 6|Set-Content -LiteralPath $taskBackup -Encoding UTF8
          try {foreach($task in $tasks){if([bool]$task.enabled){Disable-ScheduledTask -TaskName ([string]$task.taskName) -TaskPath ([string]$task.taskPath) -ErrorAction Stop|Out-Null}}}catch{foreach($task in $tasks){try{if([bool]$task.enabled){Enable-ScheduledTask -TaskName ([string]$task.taskName) -TaskPath ([string]$task.taskPath) -ErrorAction Stop|Out-Null}}catch{}};throw}
          $b=Read-Backup $taskBackup;$b.enabled=$true;$b|ConvertTo-Json -Depth 6|Set-Content -LiteralPath $taskBackup -Encoding UTF8;Add-Log 'Disabled allowlisted Windows compatibility telemetry tasks'
          $msg=('Updated '+$tasks.Count+' compatibility task(s).');if($restoreWarning){$msg+=' '+$restoreWarning};Write-Result @{ok=$true;message=$msg};break
        } else {
          if(!$b -or !$b.enabled){Write-Result @{ok=$true;message='No compatibility telemetry task changes are active.'};break}
          foreach($task in @($b.tasks)){if([bool]$task.enabled){Enable-ScheduledTask -TaskName ([string]$task.taskName) -TaskPath ([string]$task.taskPath) -ErrorAction Stop|Out-Null}else{Disable-ScheduledTask -TaskName ([string]$task.taskName) -TaskPath ([string]$task.taskPath) -ErrorAction Stop|Out-Null}}
          $b.enabled=$false;$b|ConvertTo-Json -Depth 6|Set-Content -LiteralPath $taskBackup -Encoding UTF8;Add-Log 'Restored compatibility telemetry task states';Write-Result @{ok=$true;message='Original compatibility task states restored.'};break
        }
      }
      $regPath=Get-RegPath ([string]$t.key)
      $backupDir=Join-Path $req.DataDir 'backups'; New-Item -ItemType Directory -Path $backupDir -Force | Out-Null
      $backup=Join-Path $backupDir ($id+'.json')
      if([bool]$req.Enable) {
        $restoreWarning=Ensure-RestorePoint
        if(!(Test-Path -LiteralPath $backup)) {
          $exists=Test-Path -LiteralPath $regPath
          $present=$false;$old=$null;$oldKind=$null
          if($exists) { $keyObj=Get-Item -LiteralPath $regPath; if($keyObj.GetValueNames() -contains [string]$t.name) { $present=$true;$old=$keyObj.GetValue([string]$t.name,$null,[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames);$oldKind=[string]$keyObj.GetValueKind([string]$t.name) } }
          @{id=$id;key=[string]$t.key;name=[string]$t.name;keyExisted=$exists;valueExisted=$present;value=$old;kind=$oldKind;enabled=$false} | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $backup -Encoding UTF8
        }
        New-Item -Path $regPath -Force | Out-Null
        Set-RegValue $regPath ([string]$t.name) $t.value ([string]$t.kind)
        $b=Read-Backup $backup; $b.enabled=$true; $b | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $backup -Encoding UTF8
        Add-Log ('Applied '+$id)
        $msg='Change applied and the original registry state was saved.'; if($restoreWarning){$msg+=' '+$restoreWarning}
        Write-Result @{ok=$true;message=$msg}; break
      } else {
        $b=Read-Backup $backup; if(!$b){Write-Result @{ok=$true;message='No saved state exists; nothing to restore.'};break}
        $bp=Get-RegPath ([string]$b.key)
        if([bool]$b.valueExisted) {
          New-Item -Path $bp -Force | Out-Null
          Set-RegValue $bp ([string]$b.name) $b.value ([string]$b.kind)
        } else { if(Test-Path -LiteralPath $bp) { Remove-ItemProperty -LiteralPath $bp -Name ([string]$b.name) -ErrorAction SilentlyContinue } }
        Remove-CreatedEmptyKey $b
        $b.enabled=$false; $b | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $backup -Encoding UTF8; Add-Log ('Reverted '+$id)
        Write-Result @{ok=$true;message='Saved registry state restored.'}; break
      }
    }
    'RevertAll' {
      $count=0
      $backupDir=Join-Path $req.DataDir 'backups'
      if(Test-Path -LiteralPath $backupDir) { Get-ChildItem -LiteralPath $backupDir -Filter 'safe-*.json' -File | ForEach-Object {
        $b=Read-Backup $_.FullName; if($b -and $b.enabled) { $p=Get-RegPath ([string]$b.key); if($b.valueExisted) { Set-RegValue $p ([string]$b.name) $b.value ([string]$b.kind) } elseif(Test-Path -LiteralPath $p) { Remove-ItemProperty -LiteralPath $p -Name ([string]$b.name) -ErrorAction SilentlyContinue };Remove-CreatedEmptyKey $b;$b.enabled=$false;$b|ConvertTo-Json -Depth 6|Set-Content -LiteralPath $_.FullName -Encoding UTF8;$count++ }
      } }
      $startupDir=Join-Path $req.DataDir 'startup';if(Test-Path -LiteralPath $startupDir){Get-ChildItem -LiteralPath $startupDir -Filter '*.json' -File|ForEach-Object{$b=Read-Backup $_.FullName;if($b){Set-RegValue (Get-RegPath ([string]$b.key)) ([string]$b.name) $b.value ([string]$b.kind);Remove-Item -LiteralPath $_.FullName -Force;$count++}}}
      $serviceDir=Join-Path $req.DataDir 'services';if(Test-Path -LiteralPath $serviceDir){Get-ChildItem -LiteralPath $serviceDir -Filter '*.json' -File|ForEach-Object{$b=Read-Backup $_.FullName;if($b -and $b.enabled){Restore-ServiceBackup $b;$b.enabled=$false;$b|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $_.FullName -Encoding UTF8;$count++}}}
      $nagleDir=Join-Path $req.DataDir 'nagle';if(Test-Path -LiteralPath $nagleDir){Get-ChildItem -LiteralPath $nagleDir -Filter '*.json' -File|ForEach-Object{$b=Read-Backup $_.FullName;if($b -and $b.enabled){foreach($entry in @($b.interfaces)){$path=Get-RegPath ([string]$entry.key);foreach($value in @($entry.values)){if([bool]$value.present){Set-RegValue $path ([string]$value.name) $value.value ([string]$value.kind)}elseif(Test-Path -LiteralPath $path){Remove-ItemProperty -LiteralPath $path -Name ([string]$value.name) -ErrorAction SilentlyContinue}}};$b.enabled=$false;$b|ConvertTo-Json -Depth 8|Set-Content -LiteralPath $_.FullName -Encoding UTF8;$count++}}}
      $hibernateDir=Join-Path $req.DataDir 'hibernation';if(Test-Path -LiteralPath $hibernateDir){Get-ChildItem -LiteralPath $hibernateDir -Filter '*.json' -File|ForEach-Object{$b=Read-Backup $_.FullName;if($b -and $b.enabled){Set-HibernationEnabled ([bool]$b.wasEnabled);$b.enabled=$false;$b|ConvertTo-Json -Depth 4|Set-Content -LiteralPath $_.FullName -Encoding UTF8;$count++}}}
      $compatTaskDir=Join-Path $req.DataDir 'compatibilitytasks';if(Test-Path -LiteralPath $compatTaskDir){Get-ChildItem -LiteralPath $compatTaskDir -Filter '*.json' -File|ForEach-Object{$b=Read-Backup $_.FullName;if($b -and $b.enabled){foreach($task in @($b.tasks)){if([bool]$task.enabled){Enable-ScheduledTask -TaskName ([string]$task.taskName) -TaskPath ([string]$task.taskPath) -ErrorAction Stop|Out-Null}else{Disable-ScheduledTask -TaskName ([string]$task.taskName) -TaskPath ([string]$task.taskPath) -ErrorAction Stop|Out-Null}};$b.enabled=$false;$b|ConvertTo-Json -Depth 6|Set-Content -LiteralPath $_.FullName -Encoding UTF8;$count++}}}
      $powerPlanDir=Join-Path $req.DataDir 'powerplans';if(Test-Path -LiteralPath $powerPlanDir){Get-ChildItem -LiteralPath $powerPlanDir -Filter '*.json' -File|ForEach-Object{$b=Read-Backup $_.FullName;if($b -and $b.enabled){if(Test-PowerScheme ([string]$b.originalScheme)){if((Get-ActivePowerScheme) -eq [string]$b.targetScheme){Set-ActivePowerScheme ([string]$b.originalScheme)};if([bool]$b.createdByApp){Remove-PowerScheme ([string]$b.targetScheme)};$b.enabled=$false;$b|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $_.FullName -Encoding UTF8;$count++}}}}
      $powerSettingDir=Join-Path $req.DataDir 'powersettings';if(Test-Path -LiteralPath $powerSettingDir){Get-ChildItem -LiteralPath $powerSettingDir -Filter '*.json' -File|ForEach-Object{$b=Read-Backup $_.FullName;if($b -and $b.enabled){$activate=((Get-ActivePowerScheme) -eq [string]$b.scheme);Set-PowerSettingIndices ([string]$b.scheme) ([string]$b.subgroup) ([string]$b.setting) ([uint32]$b.oldAc) ([uint32]$b.oldDc) $activate;$b.enabled=$false;$b|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $_.FullName -Encoding UTF8;$count++}}}
      $dnsDir=Join-Path $req.DataDir 'dns';if(Test-Path -LiteralPath $dnsDir){Get-ChildItem -LiteralPath $dnsDir -Filter '*.json' -File|ForEach-Object{$b=Read-Backup $_.FullName;if($b){Restore-DnsBackup $b;Remove-Item -LiteralPath $_.FullName -Force;$count++}}}
      $scheduledDir=Join-Path $req.DataDir 'scheduled';if(Test-Path -LiteralPath $scheduledDir){Get-ChildItem -LiteralPath $scheduledDir -Filter '*.json' -File|ForEach-Object{$b=Read-Backup $_.FullName;if($b -and $b.enabled){Enable-ScheduledTask -TaskName ([string]$b.taskName) -TaskPath ([string]$b.taskPath) -ErrorAction Stop|Out-Null;Remove-Item -LiteralPath $_.FullName -Force;$count++}}}
      Add-Log ('Reverted all app-managed tweaks ('+$count+')'); Write-Result @{ok=$true;message=('Restored '+$count+' app-managed tweak(s).')}; break
    }
    'CleanScan' {
      $roots=@(Get-CleanupRoots)
      $cutoff=(Get-Date).AddDays(-7);$files=@()
      foreach($root in $roots) {if($files.Count -ge 2000){break};$take=2000-$files.Count;Get-ChildItem -LiteralPath $root -File -Recurse -Force -ErrorAction SilentlyContinue | Where-Object { $_.LastWriteTime -lt $cutoff -and !($_.Attributes -band [IO.FileAttributes]::ReparsePoint) } | Select-Object -First $take | ForEach-Object {$files += [pscustomobject]@{path=$_.FullName;size=$_.Length;modified=$_.LastWriteTime.ToString('s')}} }
      Write-Result @{ok=$true;data=@($files)};break
    }
    'CleanRun' {
      $roots=@(Get-CleanupRoots)|ForEach-Object {[IO.Path]::GetFullPath($_).TrimEnd('\')}
      $removed=0;$total=0;$cutoff=(Get-Date).AddDays(-7)
      foreach($candidate in @($req.Paths)) {
        $full=[IO.Path]::GetFullPath([string]$candidate);$allowed=$false
        foreach($root in $roots) { if($full.StartsWith($root+'\',[StringComparison]::OrdinalIgnoreCase)){$allowed=$true;break} }
        if(!$allowed -or !(Test-Path -LiteralPath $full -PathType Leaf)) { continue }
        $item=Get-Item -LiteralPath $full -Force; if($item.LastWriteTime -ge $cutoff -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {continue}
        try {$total+=$item.Length;Remove-Item -LiteralPath $full -Force -ErrorAction Stop;$removed++} catch {}
      }
      Add-Log ('Removed '+$removed+' old temp files');Write-Result @{ok=$true;removed=$removed;message=('Removed '+$removed+' files ('+[math]::Round($total/1MB,1)+' MB).')};break
    }
    default { throw 'Unknown action.' }
  }
} catch { Write-Result @{ok=$false;message=$_.Exception.Message;code='ENGINE_ERROR'}; exit 1 }
