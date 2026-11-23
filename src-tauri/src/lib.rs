use serde::Serialize;
use serde_json::{Map, Value, json};
use std::os::windows::process::CommandExt;
use std::{
    collections::HashSet, fs, path::PathBuf, process::Command, sync::OnceLock, time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

const CATALOG_JSON: &str = include_str!("../../src/shared/catalog.json");
const FREE_TWEAK_LIMIT: usize = 15;
static CATALOG: OnceLock<Vec<Value>> = OnceLock::new();

fn catalog() -> &'static [Value] {
    CATALOG.get_or_init(|| {
        serde_json::from_str(CATALOG_JSON).expect("embedded tweak catalog is valid JSON")
    })
}

fn error(message: impl Into<String>) -> Value {
    json!({"ok": false, "message": message.into()})
}

fn result_array(reply: Value) -> Value {
    if reply.get("ok").and_then(Value::as_bool) == Some(false) {
        return reply;
    }
    reply.get("data").cloned().unwrap_or_else(|| json!([]))
}

fn data_dir() -> Result<PathBuf, String> {
    let local = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or("LOCALAPPDATA is unavailable")?;
    let path = local.join("NorthstarPcCare");
    fs::create_dir_all(&path)
        .map_err(|e| format!("Could not create Northstar data folder: {e}"))?;
    Ok(path)
}

fn resource_or_dev(app: &AppHandle, resource: &str, dev: &str) -> Result<PathBuf, String> {
    if let Ok(root) = app.path().resource_dir() {
        let candidate = root.join(resource);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let candidate = root.join(dev);
    candidate
        .is_file()
        .then_some(candidate)
        .ok_or_else(|| format!("Northstar resource was not found: {resource}"))
}

fn run_system(app: &AppHandle, payload: Value) -> Result<Value, String> {
    let script = resource_or_dev(
        app,
        "powershell/SystemOps.ps1",
        "electron/powershell/SystemOps.ps1",
    )?;
    let temp = std::env::temp_dir().join(format!("northstar-{}", Uuid::new_v4()));
    fs::create_dir(&temp).map_err(|e| format!("Could not create an operation folder: {e}"))?;
    let input = temp.join("request.json");
    let output = temp.join("response.json");
    let mut body = payload.as_object().cloned().unwrap_or_default();
    body.insert(
        "DataDir".into(),
        json!(data_dir()?.to_string_lossy().to_string()),
    );
    body.insert(
        "OutputPath".into(),
        json!(output.to_string_lossy().to_string()),
    );
    fs::write(
        &input,
        serde_json::to_vec(&Value::Object(body)).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("Could not prepare the Windows operation: {e}"))?;

    let result = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(script)
        .arg("-PayloadPath")
        .arg(&input)
        .creation_flags(0x08000000)
        .output();

    let reply = match result {
        Ok(_process) if output.is_file() => fs::read(&output)
            .ok()
            .and_then(|data| serde_json::from_slice::<Value>(&data).ok())
            .ok_or_else(|| "The Windows operation returned an unreadable result.".to_string()),
        Ok(process) => {
            let stderr = String::from_utf8_lossy(&process.stderr).trim().to_string();
            Err(if !stderr.is_empty() {
                stderr
            } else if process.status.success() {
                "PowerShell returned no result.".into()
            } else {
                "The Windows operation failed.".into()
            })
        }
        Err(e) => Err(format!("Could not start Windows PowerShell: {e}")),
    };
    let _ = fs::remove_dir_all(temp);
    reply
}

fn item_by_id(id: &str) -> Option<&'static Value> {
    catalog()
        .iter()
        .find(|item| item.get("id").and_then(Value::as_str) == Some(id))
}

fn supported(item: &Value) -> bool {
    let kind = item.get("kind").and_then(Value::as_str).unwrap_or("");
    let has_registry = item.get("key").and_then(Value::as_str).is_some()
        && item.get("name").and_then(Value::as_str).is_some();
    has_registry
        || (kind == "Service" && item.get("serviceName").and_then(Value::as_str).is_some())
        || kind == "PowerPlan"
        || (kind == "PowerSetting" && item.get("powerSetting").and_then(Value::as_str).is_some())
        || matches!(kind, "NetworkNagle" | "Hibernate" | "CompatibilityTasks")
}

fn allowed_tweak(id: &str) -> Result<&'static Value, String> {
    let item = item_by_id(id).ok_or("Unknown tweak ID.")?;
    if item.get("applicable").and_then(Value::as_bool) != Some(true)
        || item.get("risk").and_then(Value::as_str) == Some("High")
        || !supported(item)
    {
        return Err(
            "This catalog entry is review-only or not supported by the current Windows engine."
                .into(),
        );
    }
    Ok(item)
}

fn tweak_payload(item: &Value) -> Value {
    let keys = [
        "key",
        "name",
        "kind",
        "value",
        "serviceName",
        "powerSetting",
        "powerAc",
        "powerDc",
    ];
    let mut tweak = Map::new();
    for key in keys {
        if let Some(value) = item.get(key) {
            tweak.insert(key.to_string(), value.clone());
        }
    }
    Value::Object(tweak)
}

fn active_tweaks(app: &AppHandle) -> Result<HashSet<String>, String> {
    let reply = run_system(app, json!({"Action":"Snapshots"}))?;
    if reply.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(reply
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Could not read tweak snapshots.")
            .into());
    }
    let known: HashSet<&str> = catalog()
        .iter()
        .filter_map(|x| x.get("id").and_then(Value::as_str))
        .collect();
    Ok(reply
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|x| x.get("enabled").and_then(Value::as_bool) == Some(true))
        .filter_map(|x| x.get("id").and_then(Value::as_str))
        .filter(|id| known.contains(id))
        .map(str::to_string)
        .collect())
}

fn billing_source(app: &AppHandle) -> Result<PathBuf, String> {
    resource_or_dev(app, "billing-source.json", "electron/billing-source.json")
}

fn billing_base(app: &AppHandle) -> Result<Option<String>, String> {
    let path = billing_source(app)?;
    let config: Value = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let raw = config
        .get("baseUrl")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .trim_end_matches('/');
    if raw.is_empty() {
        return Ok(None);
    }
    if !raw.starts_with("https://") && !raw.starts_with("http://127.0.0.1") {
        return Err("Billing service URL must use HTTPS.".into());
    }
    Ok(Some(raw.to_string()))
}

fn installation_id() -> Result<String, String> {
    let file = data_dir()?.join("billing-install-id");
    if let Ok(value) = fs::read_to_string(&file) {
        let saved = value.trim();
        if Uuid::parse_str(saved).is_ok() {
            return Ok(saved.to_string());
        }
    }
    let id = Uuid::new_v4().to_string();
    fs::write(file, &id)
        .map_err(|e| format!("Could not save the Northstar installation ID: {e}"))?;
    Ok(id)
}

fn billing_status(app: &AppHandle) -> Value {
    let base = match billing_base(app) {
        Ok(Some(url)) => url,
        Ok(None) => return json!({"configured":false,"pro":false,"plan":null}),
        Err(e) => return json!({"configured":false,"pro":false,"plan":null,"error":e}),
    };
    let id = match installation_id() {
        Ok(x) => x,
        Err(e) => return json!({"configured":true,"pro":false,"plan":null,"error":e}),
    };
    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .and_then(|client| client.get(format!("{base}/v1/entitlement/{id}")).send());
    match response {
        Ok(response) if response.status().is_success() => match response.json::<Value>() {
            Ok(data) => {
                json!({"configured":true,"pro":data.get("pro").and_then(Value::as_bool)==Some(true),"plan":data.get("plan").cloned().unwrap_or(Value::Null)})
            }
            Err(e) => json!({"configured":true,"pro":false,"plan":null,"error":e.to_string()}),
        },
        Ok(response) => {
            json!({"configured":true,"pro":false,"plan":null,"error":format!("Billing service returned HTTP {}.", response.status())})
        }
        Err(e) => json!({"configured":true,"pro":false,"plan":null,"error":e.to_string()}),
    }
}

fn has_pro(app: &AppHandle) -> bool {
    billing_status(app).get("pro").and_then(Value::as_bool) == Some(true)
}

fn free_limit(app: &AppHandle, ids: &[String]) -> Result<(), String> {
    let active = active_tweaks(app)?;
    let additional = ids.iter().filter(|id| !active.contains(*id)).count();
    if !has_pro(app) && active.len() + additional > FREE_TWEAK_LIMIT {
        return Err(format!(
            "Free includes up to {FREE_TWEAK_LIMIT} active tweaks. Remove tweaks or choose Pro."
        ));
    }
    Ok(())
}

#[derive(Clone, Serialize)]
struct ProfileProgress {
    completed: usize,
    total: usize,
    current: String,
}

fn progress(app: &AppHandle, completed: usize, total: usize, current: impl Into<String>) {
    let _ = app.emit(
        "profile-progress",
        ProfileProgress {
            completed,
            total,
            current: current.into(),
        },
    );
}

fn run_tweak(
    app: &AppHandle,
    id: &str,
    enable: bool,
    create_restore_point: bool,
) -> Result<Value, String> {
    let item = allowed_tweak(id)?;
    if enable {
        free_limit(app, &[id.to_string()])?;
    }
    run_system(
        app,
        json!({"Action":"Tweak","Id":id,"Enable":enable,"CreateRestorePoint":create_restore_point,"Tweak":tweak_payload(item)}),
    )
}

fn bridge_call(app: &AppHandle, method: &str, args: Value) -> Result<Value, String> {
    match method {
        "getMetrics" => Ok(run_system(app, json!({"Action":"Metrics"}))?
            .get("data")
            .cloned()
            .unwrap_or(Value::Null)),
        "getSnapshots" => Ok(result_array(run_system(
            app,
            json!({"Action":"Snapshots"}),
        )?)),
        "runTweak" => {
            let id = args
                .get("id")
                .and_then(Value::as_str)
                .ok_or("Invalid tweak ID.")?;
            let enable = args
                .get("enable")
                .and_then(Value::as_bool)
                .ok_or("Invalid toggle value.")?;
            if enable && item_by_id(id).is_none() {
                return Ok(error("Unknown tweak ID."));
            }
            run_tweak(
                app,
                id,
                enable,
                args.get("createRestorePoint")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            )
        }
        "runTweakBatch" => {
            let ids = args
                .get("ids")
                .and_then(Value::as_array)
                .ok_or("Invalid optimization profile.")?;
            if ids.is_empty() || ids.len() > 20 {
                return Ok(error("Invalid optimization profile."));
            }
            let ids: Vec<String> = ids
                .iter()
                .map(|x| x.as_str().map(str::to_string).ok_or("Invalid tweak ID."))
                .collect::<Result<_, _>>()?;
            if ids.iter().collect::<HashSet<_>>().len() != ids.len() {
                return Ok(error("Duplicate tweak in this profile."));
            }
            let mut items = Vec::new();
            for id in &ids {
                let item = allowed_tweak(id)?;
                if item.get("risk").and_then(Value::as_str) != Some("Low")
                    || !matches!(
                        item.get("kind").and_then(Value::as_str).unwrap_or(""),
                        "DWord" | "String"
                    )
                {
                    return Ok(error(
                        "The safe optimization profile contains an unsupported tweak.",
                    ));
                }
                items.push(json!({"id":id,"tweak":tweak_payload(item)}));
            }
            free_limit(app, &ids)?;
            run_system(
                app,
                json!({"Action":"TweakBatch","CreateRestorePoint":args.get("createRestorePoint").and_then(Value::as_bool).unwrap_or(false),"Items":items}),
            )
        }
        "runProfile" => {
            let raw = args
                .get("ids")
                .and_then(Value::as_array)
                .ok_or("Invalid profile data.")?;
            if raw.len() > catalog().len() {
                return Ok(error("This profile has too many tweaks."));
            }
            let ids: Vec<String> = raw
                .iter()
                .map(|x| x.as_str().map(str::to_string).ok_or("Invalid tweak ID."))
                .collect::<Result<_, _>>()?;
            if ids.iter().collect::<HashSet<_>>().len() != ids.len() {
                return Ok(error("This profile contains duplicate tweaks."));
            }
            let mut items = Vec::new();
            for id in &ids {
                items.push((id.clone(), allowed_tweak(id)?.clone()));
            }
            let active = active_tweaks(app)?;
            let pending: Vec<_> = items
                .into_iter()
                .filter(|(id, _)| !active.contains(id))
                .collect();
            free_limit(
                app,
                &pending.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(),
            )?;
            if pending.is_empty() {
                progress(app, 1, 1, "Profile is already applied");
                return Ok(json!({"ok":true,"message":"This profile is already applied."}));
            }
            let total = pending.len();
            let create_restore_point = args
                .get("createRestorePoint")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            for (index, (id, item)) in pending.into_iter().enumerate() {
                let title = item
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or(&id)
                    .to_string();
                progress(app, index, total, title.clone());
                let result = run_system(
                    app,
                    json!({"Action":"Tweak","Id":id,"Enable":true,"CreateRestorePoint":create_restore_point && index==0,"Tweak":tweak_payload(&item)}),
                );
                match result {
                    Ok(reply) if reply.get("ok").and_then(Value::as_bool) == Some(true) => {
                        progress(app, index + 1, total, title)
                    }
                    Ok(reply) => {
                        let message = reply
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("The Windows engine rejected a tweak.");
                        progress(app, index, total, format!("Stopped at {title}"));
                        return Ok(error(format!(
                            "Applied {index} of {total} profile tweaks. {title}: {message}"
                        )));
                    }
                    Err(message) => {
                        return Ok(error(format!(
                            "Applied {index} of {total} profile tweaks. {title}: {message}"
                        )));
                    }
                }
            }
            Ok(
                json!({"ok":true,"message":format!("Applied {total} profile tweaks. Each change has an app-managed backup.")}),
            )
        }
        "revertAll" => run_system(app, json!({"Action":"RevertAll"})),
        "getStartup" => Ok(result_array(run_system(
            app,
            json!({"Action":"StartupScan"}),
        )?)),
        "setStartup" => set_startup(app, args, false),
        "getScheduledStartup" => Ok(result_array(run_system(
            app,
            json!({"Action":"ScheduledStartupScan"}),
        )?)),
        "setScheduledStartup" => set_startup(app, args, true),
        "getDnsAdapters" => Ok(result_array(run_system(app, json!({"Action":"DnsScan"}))?)),
        "setDns" => {
            let index = args.get("index").and_then(Value::as_u64).filter(|x| (1..=65535).contains(x)).ok_or("The selected adapter ID is invalid. Refresh the adapter list and select it again.")?;
            let provider = args
                .get("provider")
                .and_then(Value::as_str)
                .filter(|x| matches!(*x, "cloudflare" | "google"))
                .ok_or("Invalid DNS provider.")?;
            run_system(
                app,
                json!({"Action":"DnsSet","InterfaceIndex":index,"Provider":provider}),
            )
        }
        "restoreDns" => {
            let index = args
                .get("index")
                .and_then(Value::as_u64)
                .filter(|x| (1..=65535).contains(x))
                .ok_or("Invalid DNS adapter.")?;
            run_system(app, json!({"Action":"DnsRestore","InterfaceIndex":index}))
        }
        "flushDns" => run_system(app, json!({"Action":"DnsFlush"})),
        "scanJunk" => {
            if !has_pro(app) {
                return Ok(error("Cleaner is part of Pro."));
            }
            let reply = run_system(app, json!({"Action":"CleanScan"}))?;
            if reply.get("ok").and_then(Value::as_bool) == Some(true) {
                Ok(result_array(reply))
            } else {
                Ok(reply)
            }
        }
        "cleanJunk" => {
            if !has_pro(app) {
                return Ok(error("Cleaner is part of Pro."));
            }
            let paths = args
                .get("paths")
                .and_then(Value::as_array)
                .ok_or("Invalid cleanup selection.")?;
            if paths.len() > 2000
                || paths
                    .iter()
                    .any(|p| p.as_str().is_none_or(|s| s.len() > 32767))
            {
                return Ok(error("Invalid cleanup selection."));
            }
            run_system(app, json!({"Action":"CleanRun","Paths":paths}))
        }
        "getBillingStatus" => Ok(billing_status(app)),
        "startCheckout" => checkout(app, args),
        _ => Err(format!("Unknown Northstar command: {method}")),
    }
}

fn set_startup(app: &AppHandle, args: Value, scheduled: bool) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(Value::as_str)
        .ok_or("Invalid startup item.")?;
    let enable = args
        .get("enable")
        .and_then(Value::as_bool)
        .ok_or("Invalid startup operation.")?;
    if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(error("Invalid startup item."));
    }
    let action = if scheduled {
        "ScheduledStartupSet"
    } else {
        "StartupSet"
    };
    run_system(app, json!({"Action":action,"Id":id,"Enable":enable}))
}

fn checkout(app: &AppHandle, args: Value) -> Result<Value, String> {
    let plan = args
        .get("plan")
        .and_then(Value::as_str)
        .filter(|x| matches!(*x, "monthly" | "lifetime"))
        .ok_or("Invalid Pro plan.")?;
    let base = match billing_base(app)? {
        Some(x) => x,
        None => return Ok(error("Northstar billing is not configured yet.")),
    };
    let install_id = installation_id()?;
    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .and_then(|client| {
            client
                .post(format!("{base}/v1/checkout"))
                .json(&json!({"installId":install_id,"plan":plan}))
                .send()
        })
        .map_err(|e| format!("Could not reach the billing service: {e}"))?;
    let status = response.status();
    let body: Value = response
        .json()
        .map_err(|e| format!("Billing service returned invalid data: {e}"))?;
    if !status.is_success() {
        return Ok(error(
            body.get("error")
                .and_then(Value::as_str)
                .unwrap_or("Billing service rejected checkout."),
        ));
    }
    let url = body
        .get("url")
        .and_then(Value::as_str)
        .filter(|s| s.starts_with("https://checkout.stripe.com/"))
        .ok_or("Billing service returned an invalid checkout URL.")?;
    open::that(url).map_err(|e| format!("Could not open Stripe Checkout: {e}"))?;
    Ok(json!({"ok":true,"message":"Stripe Checkout opened in your browser."}))
}

#[tauri::command]
fn bridge(method: String, args: Value, app: AppHandle) -> Result<Value, String> {
    bridge_call(&app, &method, args)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(tauri::generate_handler![bridge])
        .run(tauri::generate_context!())
        .expect("failed to start Northstar PC Care");
}
