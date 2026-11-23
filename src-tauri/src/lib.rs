use serde::Serialize;
use serde_json::{Map, Value, json};
use std::os::windows::process::CommandExt;
use std::{
    collections::HashSet, fs, path::PathBuf, process::Command, sync::OnceLock, time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;
use windows_registry::{CURRENT_USER, Key, LOCAL_MACHINE, Type as RegistryType};
use windows_result::Error as RegistryError;

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
    if let Some(reply) = run_native_system(&payload)? {
        return Ok(reply);
    }
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

fn run_native_system(payload: &Value) -> Result<Option<Value>, String> {
    match payload.get("Action").and_then(Value::as_str) {
        Some("Snapshots") => Ok(Some(json!({"ok":true,"data":registry_snapshots()?}))),
        Some("Tweak") => {
            if payload.get("CreateRestorePoint").and_then(Value::as_bool) == Some(true) {
                return Ok(None);
            }
            let tweak = payload.get("Tweak").ok_or("Invalid tweak payload.")?;
            if !matches!(
                tweak.get("kind").and_then(Value::as_str),
                Some("DWord" | "String")
            ) {
                return Ok(None);
            }
            Ok(Some(apply_registry_tweak(payload)?))
        }
        Some("TweakBatch") => {
            if payload.get("CreateRestorePoint").and_then(Value::as_bool) == Some(true) {
                return Ok(None);
            }
            let items = payload
                .get("Items")
                .and_then(Value::as_array)
                .ok_or("Invalid optimization profile.")?;
            if items.is_empty() || items.len() > 20 {
                return Ok(Some(error("Invalid optimization profile.")));
            }
            let mut applied = 0;
            for item in items {
                let reply = apply_registry_tweak(
                    &json!({"Id":item.get("id"),"Enable":true,"Tweak":item.get("tweak")}),
                )?;
                if reply.get("ok").and_then(Value::as_bool) != Some(true) {
                    let title = item.get("id").and_then(Value::as_str).unwrap_or("tweak");
                    let message = reply
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("The registry operation failed.");
                    return Ok(Some(error(format!(
                        "Profile applied {applied} of {} settings before {title} failed: {message}",
                        items.len()
                    ))));
                }
                applied += 1;
            }
            Ok(Some(
                json!({"ok":true,"message":format!("Applied {applied} reviewed settings. Each change has an app-managed backup.")}),
            ))
        }
        _ => Ok(None),
    }
}

fn valid_registry_path(path: &str) -> bool {
    let Some((hive, subkey)) = path.split_once('\\') else {
        return false;
    };
    matches!(hive, "HKCU" | "HKLM")
        && !subkey.is_empty()
        && subkey.split('\\').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part.chars().all(|c| {
                    c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '-' | '.' | '{' | '}')
                })
        })
}

fn registry_key_raw(path: &str, create: bool) -> windows_registry::Result<Key> {
    let (hive, subkey) = path.split_once('\\').expect("validated registry path");
    let root = if hive == "HKCU" {
        CURRENT_USER
    } else {
        LOCAL_MACHINE
    };
    if create {
        root.create(subkey)
    } else {
        root.open(subkey)
    }
}

fn registry_key(path: &str, create: bool) -> Result<Key, String> {
    if !valid_registry_path(path) {
        return Err("Registry path is outside the supported hives.".into());
    }
    registry_key_raw(path, create).map_err(|e| e.to_string())
}

fn registry_not_found(error: &RegistryError) -> bool {
    error.code().0 == 0x8007_0002u32 as i32 || error.code().0 == 0x8007_0003u32 as i32
}

fn registry_type_code(kind: &str) -> Result<u32, String> {
    match kind {
        "None" => Ok(0),
        "String" => Ok(1),
        "ExpandString" => Ok(2),
        "Binary" => Ok(3),
        "DWord" => Ok(4),
        "DWordBigEndian" => Ok(5),
        "Link" => Ok(6),
        "MultiString" => Ok(7),
        "ResourceList" => Ok(8),
        "FullResourceDescriptor" => Ok(9),
        "ResourceRequirementsList" => Ok(10),
        "QWord" => Ok(11),
        _ => Err(format!("Unsupported saved registry value type: {kind}.")),
    }
}

fn registry_json(kind: RegistryType, bytes: &[u8]) -> Value {
    match kind {
        RegistryType::U32 if bytes.len() == 4 => {
            json!(u32::from_le_bytes(
                bytes.try_into().expect("checked length")
            ))
        }
        RegistryType::U64 if bytes.len() == 8 => {
            json!(u64::from_le_bytes(
                bytes.try_into().expect("checked length")
            ))
        }
        RegistryType::String | RegistryType::ExpandString => {
            let words: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|x| u16::from_le_bytes([x[0], x[1]]))
                .take_while(|x| *x != 0)
                .collect();
            json!(String::from_utf16_lossy(&words))
        }
        RegistryType::Other(7) => {
            let words: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|x| u16::from_le_bytes([x[0], x[1]]))
                .collect();
            let parts: Vec<_> = words
                .split(|x| *x == 0)
                .filter(|x| !x.is_empty())
                .map(String::from_utf16_lossy)
                .collect();
            json!(parts)
        }
        RegistryType::Bytes | RegistryType::Other(_) => {
            json!(bytes.iter().copied().collect::<Vec<_>>())
        }
        _ => Value::Null,
    }
}

fn legacy_registry_bytes(kind: &str, value: &Value) -> Result<(RegistryType, Vec<u8>), String> {
    let code = registry_type_code(kind)?;
    let ty = match code {
        0 => RegistryType::Other(0),
        1 => RegistryType::String,
        2 => RegistryType::ExpandString,
        3 => RegistryType::Bytes,
        4 => RegistryType::U32,
        5..=10 => RegistryType::Other(code),
        11 => RegistryType::U64,
        _ => unreachable!(),
    };
    let bytes = match code {
        0 => Vec::new(),
        1 | 2 => {
            let text = value.as_str().ok_or("Saved registry string is invalid.")?;
            text.encode_utf16()
                .chain(std::iter::once(0))
                .flat_map(u16::to_le_bytes)
                .collect()
        }
        3 => value
            .as_array()
            .ok_or("Saved registry binary value is invalid.")?
            .iter()
            .map(|x| x.as_u64().filter(|b| *b <= 255).map(|b| b as u8))
            .collect::<Option<Vec<_>>>()
            .ok_or("Saved registry bytes are invalid.")?,
        5 => value
            .as_array()
            .and_then(|a| {
                let bytes = a
                    .iter()
                    .map(|x| x.as_u64().filter(|b| *b <= 255).map(|b| b as u8))
                    .collect::<Option<Vec<_>>>()?;
                <[u8; 4]>::try_from(bytes).ok().map(|b| b.to_vec())
            })
            .or_else(|| value.as_u64().map(|n| (n as u32).to_be_bytes().to_vec()))
            .ok_or("Saved big-endian registry value is invalid.")?,
        6 => {
            let text = value
                .as_str()
                .ok_or("Saved registry link value is invalid.")?;
            text.encode_utf16()
                .chain(std::iter::once(0))
                .flat_map(u16::to_le_bytes)
                .collect()
        }
        7 => {
            let parts = value
                .as_array()
                .ok_or("Saved registry multi-string is invalid.")?;
            let mut words = Vec::new();
            for part in parts {
                words.extend(
                    part.as_str()
                        .ok_or("Saved registry multi-string item is invalid.")?
                        .encode_utf16(),
                );
                words.push(0);
            }
            words.push(0);
            words.into_iter().flat_map(u16::to_le_bytes).collect()
        }
        8..=10 => value
            .as_array()
            .ok_or("Saved registry resource value is invalid.")?
            .iter()
            .map(|x| x.as_u64().filter(|b| *b <= 255).map(|b| b as u8))
            .collect::<Option<Vec<_>>>()
            .ok_or("Saved registry resource bytes are invalid.")?,
        4 => (value.as_u64().ok_or("Saved registry number is invalid.")? as u32)
            .to_le_bytes()
            .to_vec(),
        11 => value
            .as_u64()
            .ok_or("Saved registry number is invalid.")?
            .to_le_bytes()
            .to_vec(),
        _ => unreachable!(),
    };
    Ok((ty, bytes))
}

fn read_registry_value(path: &str, name: &str) -> Result<Option<(u32, Vec<u8>)>, String> {
    if !valid_registry_path(path) {
        return Err("Registry path is outside the supported hives.".into());
    }
    let key = match registry_key_raw(path, false) {
        Ok(key) => key,
        Err(error) if registry_not_found(&error) => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let Some((_, value)) = key
        .values()
        .map_err(|e| e.to_string())?
        .find(|(saved_name, _)| saved_name.eq_ignore_ascii_case(name))
    else {
        return Ok(None);
    };
    Ok(Some((u32::from(value.ty()), value.as_ref().to_vec())))
}

fn set_registry_bytes(path: &str, name: &str, code: u32, bytes: &[u8]) -> Result<(), String> {
    let ty = match code {
        0 => RegistryType::Other(0),
        1 => RegistryType::String,
        2 => RegistryType::ExpandString,
        3 => RegistryType::Bytes,
        4 => RegistryType::U32,
        11 => RegistryType::U64,
        n => RegistryType::Other(n),
    };
    registry_key(path, true)?
        .set_bytes(name, ty, bytes)
        .map_err(|e| e.to_string())
}

fn write_json(path: &std::path::Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let temp = path.with_extension("tmp");
    fs::write(&temp, serde_json::to_vec(value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    fs::rename(&temp, path).map_err(|e| e.to_string())
}

fn registry_backup_path(id: &str) -> Result<PathBuf, String> {
    if id.len() > 32 || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') {
        return Err("Invalid tweak ID.".into());
    }
    Ok(data_dir()?.join("backups").join(format!("{id}.json")))
}

fn apply_registry_tweak(payload: &Value) -> Result<Value, String> {
    let id = payload
        .get("Id")
        .and_then(Value::as_str)
        .ok_or("Invalid tweak ID.")?;
    let enable = payload
        .get("Enable")
        .and_then(Value::as_bool)
        .ok_or("Invalid toggle value.")?;
    let tweak = payload.get("Tweak").ok_or("Invalid tweak payload.")?;
    let path = tweak
        .get("key")
        .and_then(Value::as_str)
        .ok_or("Invalid registry path.")?;
    let name = tweak
        .get("name")
        .and_then(Value::as_str)
        .filter(|x| !x.is_empty() && !x.contains('\\') && !x.contains('\0'))
        .ok_or("Invalid registry value name.")?;
    let backup_path = registry_backup_path(id)?;
    if enable {
        let kind = tweak
            .get("kind")
            .and_then(Value::as_str)
            .ok_or("Invalid registry value type.")?;
        let (type_code, bytes) = match kind {
            "DWord" => {
                let number = tweak
                    .get("value")
                    .and_then(Value::as_u64)
                    .ok_or("Invalid DWord value.")? as u32;
                (4, number.to_le_bytes().to_vec())
            }
            "String" => {
                let text = tweak
                    .get("value")
                    .and_then(Value::as_str)
                    .ok_or("Invalid string value.")?;
                (
                    1,
                    text.encode_utf16()
                        .chain(std::iter::once(0))
                        .flat_map(u16::to_le_bytes)
                        .collect(),
                )
            }
            _ => {
                return Err("This registry value type is not supported by the Rust engine.".into());
            }
        };
        let backup = if backup_path.exists() {
            serde_json::from_slice::<Value>(&fs::read(&backup_path).map_err(|e| e.to_string())?)
                .map_err(|_| "The saved tweak backup is invalid; it was left unchanged.")?
        } else {
            let original = read_registry_value(path, name)?;
            let key_existed = match registry_key_raw(path, false) {
                Ok(_) => true,
                Err(error) if registry_not_found(&error) => false,
                Err(error) => return Err(error.to_string()),
            };
            let (old_type, old_bytes, old_value, old_kind) = match original {
                Some((code, raw)) => {
                    let ty = match code {
                        0 => RegistryType::Other(0),
                        1 => RegistryType::String,
                        2 => RegistryType::ExpandString,
                        3 => RegistryType::Bytes,
                        4 => RegistryType::U32,
                        11 => RegistryType::U64,
                        other => RegistryType::Other(other),
                    };
                    let kind = match code {
                        0 => "None",
                        1 => "String",
                        2 => "ExpandString",
                        3 => "Binary",
                        4 => "DWord",
                        5 => "DWordBigEndian",
                        6 => "Link",
                        7 => "MultiString",
                        8 => "ResourceList",
                        9 => "FullResourceDescriptor",
                        10 => "ResourceRequirementsList",
                        11 => "QWord",
                        _ => "Binary",
                    };
                    (Some(code), Some(raw.clone()), registry_json(ty, &raw), kind)
                }
                None => (None, None, Value::Null, "DWord"),
            };
            let mut backup = json!({
                "id":id,"key":path,"name":name,"keyExisted":key_existed,
                "valueExisted":old_type.is_some(),"value":old_value,"kind":old_kind,
                "enabled":false
            });
            if let Some(code) = old_type {
                backup["registryType"] = json!(code);
                backup["rawData"] = json!(old_bytes.unwrap_or_default());
            }
            backup
        };
        write_json(&backup_path, &backup)?;
        set_registry_bytes(path, name, type_code, &bytes)?;
        let mut backup = backup;
        backup["enabled"] = json!(true);
        write_json(&backup_path, &backup)?;
        add_native_log(format!("Applied {id}"))?;
        Ok(json!({"ok":true,"message":"Change applied and the original registry state was saved."}))
    } else {
        if !backup_path.exists() {
            return Ok(json!({"ok":true,"message":"No saved state exists; nothing to restore."}));
        }
        let mut backup: Value =
            serde_json::from_slice(&fs::read(&backup_path).map_err(|e| e.to_string())?)
                .map_err(|_| "The saved tweak backup is invalid; it was left unchanged.")?;
        if backup.get("valueExisted").and_then(Value::as_bool) == Some(true) {
            if let (Some(code), Some(raw)) = (
                backup.get("registryType").and_then(Value::as_u64),
                backup.get("rawData").and_then(Value::as_array),
            ) {
                let raw = raw
                    .iter()
                    .map(|x| x.as_u64().filter(|b| *b <= 255).map(|b| b as u8))
                    .collect::<Option<Vec<_>>>()
                    .ok_or("The saved registry bytes are invalid.")?;
                set_registry_bytes(path, name, code as u32, &raw)?;
            } else {
                let kind = backup
                    .get("kind")
                    .and_then(Value::as_str)
                    .ok_or("Saved registry kind is missing.")?;
                let (ty, bytes) = legacy_registry_bytes(kind, &backup["value"])?;
                set_registry_bytes(path, name, u32::from(ty), &bytes)?;
            }
        } else {
            if !valid_registry_path(path) {
                return Err("Saved registry path is outside the supported hives.".into());
            }
            match registry_key_raw(path, false) {
                Ok(key) => {
                    if let Err(e) = key.remove_value(name) {
                        if !registry_not_found(&e) {
                            return Err(e.to_string());
                        }
                    }
                }
                Err(error) if registry_not_found(&error) => {}
                Err(error) => return Err(error.to_string()),
            }
            if backup.get("keyExisted").and_then(Value::as_bool) == Some(false) {
                remove_empty_registry_key(path)?;
            }
        }
        backup["enabled"] = json!(false);
        write_json(&backup_path, &backup)?;
        add_native_log(format!("Reverted {id}"))?;
        Ok(json!({"ok":true,"message":"Saved registry state restored."}))
    }
}

fn remove_empty_registry_key(path: &str) -> Result<(), String> {
    let Some((parent, name)) = path.rsplit_once('\\') else {
        return Ok(());
    };
    let Ok(key) = registry_key(path, false) else {
        return Ok(());
    };
    if key.values().map_err(|e| e.to_string())?.next().is_some()
        || key.keys().map_err(|e| e.to_string())?.next().is_some()
    {
        return Ok(());
    }
    drop(key);
    registry_key(parent, false)?
        .remove_tree(name)
        .map_err(|e| e.to_string())
}

fn registry_snapshots() -> Result<Vec<Value>, String> {
    let mut snapshots = Vec::new();
    let root = data_dir()?;
    for folder in [
        "backups",
        "services",
        "powerplans",
        "powersettings",
        "nagle",
        "hibernation",
        "compatibilitytasks",
    ] {
        let path = root.join(folder);
        let Ok(entries) = fs::read_dir(path) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.path().extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let Ok(bytes) = fs::read(entry.path()) else {
                continue;
            };
            let Ok(backup) = serde_json::from_slice::<Value>(&bytes) else {
                continue;
            };
            let Some(id) = backup.get("id").and_then(Value::as_str) else {
                continue;
            };
            snapshots.push(json!({"id":id,"enabled":backup.get("enabled").and_then(Value::as_bool).unwrap_or(false),"restorable":true}));
        }
    }
    Ok(snapshots)
}

fn add_native_log(message: String) -> Result<(), String> {
    let path = data_dir()?.join("operations.json");
    let mut items = match fs::read(&path)
        .ok()
        .and_then(|data| serde_json::from_slice::<Value>(&data).ok())
    {
        Some(Value::Array(items)) => items,
        Some(Value::Object(item)) => vec![Value::Object(item)],
        _ => Vec::new(),
    };
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    items.push(json!({"time":seconds,"message":message}));
    fs::write(path, serde_json::to_vec(&items).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_paths_cannot_escape_supported_hives() {
        assert!(valid_registry_path("HKCU\\Software\\Northstar"));
        assert!(valid_registry_path(
            "HKLM\\SYSTEM\\CurrentControlSet\\Services\\{01234567-89ab-cdef-0123-456789abcdef}"
        ));
        assert!(!valid_registry_path("HKCR\\Software\\Northstar"));
        assert!(!valid_registry_path("HKCU\\..\\Run"));
        assert!(!valid_registry_path("HKLM\\"));
    }

    #[test]
    fn registry_restore_codec_keeps_supported_types() {
        let (ty, dword) = legacy_registry_bytes("DWord", &json!(4294967295u64)).unwrap();
        assert_eq!(u32::from(ty), 4);
        assert_eq!(registry_json(ty, &dword), json!(4294967295u64));

        let (ty, multi) = legacy_registry_bytes("MultiString", &json!(["one", "two"])).unwrap();
        assert_eq!(u32::from(ty), 7);
        assert_eq!(registry_json(ty, &multi), json!(["one", "two"]));
    }
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
