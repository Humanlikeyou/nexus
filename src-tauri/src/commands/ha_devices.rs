//! 1118a lane B3, Smart home devices (`tasks/HANDOFF-1118a.md` sections 8.2 and 9.2; decision 0036).
//!
//! Two commands over the Home Assistant client in `ha.rs` (loopback only, token from the app store, fixed-text
//! errors that never echo the token; Home Assistant is not an outbound host, so `NEXUS_OUTBOUND_DISABLED` does
//! not apply, exactly as in `ha.rs`):
//!
//! - `ha_devices_list` (read): household devices with name, type, room and state. Rooms come from HA's REST
//!   template API (`area_name()`), because the area registry is otherwise WebSocket-only. If that call fails
//!   the list still comes back with `roomsAvailable: false` and no rooms. Attributes are a fixed whitelist of
//!   plain numbers (brightness, setpoint, volume...), so tokens, camera URLs and `entity_picture` never leave.
//!   HA's own backup / container / supervisor entities are marked `system` and are status-only.
//! - `ha_device_set` (write): ONE command, a strict domain to service allowlist, entity must be in the latest
//!   device list. Locks, alarm panels, cameras, scripts, automations, shell/rest commands, `homeassistant.*`,
//!   garage doors and gates are refused outright (no approval route). Sample entity ids are refused. The write
//!   goes through `adds::logged_write` (kind "ha"): History line plus 8 s Undo that re-applies the prior state
//!   (`add_undo("ha", id)`). Scenes can't be undone and the result says so.
//!
//! Never logs entity names, states or tokens.

use super::adds::{self, Dirs, UndoRef};
use super::goal_roadmap::clean_text;
use super::ha::{self, HaConfig};
use super::local_store::now_jst;
use super::shared::task::{blocking, JoinErr};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::HashMap;

const ERR_REFUSED: &str =
    "not_allowed: Locks, alarms, cameras, scripts, automations, garage doors and system actions are not controlled from here.";
const ERR_SAMPLE: &str = "not_allowed: Sample controls are never sent to Home Assistant.";
const ERR_UNKNOWN: &str = "invalid_input: That device is not in the Home Assistant device list.";
const ERR_STATUS_ONLY: &str = "not_allowed: That device is status-only.";
const ERR_TIMEOUT: &str = "timed_out: Home Assistant did not answer in time.";
const ERR_NOT_RECORDED: &str = "storage_error: The change was applied but not recorded in History.";
/// Cap for the HA calls on the write and Undo paths.
const REQ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
const NAME_MAX: usize = 80;
const STATE_MAX: usize = 64;

/// Template for area names: only entities that have an area; the JSON is `[[entity_id, area], ...]`.
const AREA_TEMPLATE: &str = "{% set o = [] %}{% for s in states %}{% set a = area_name(s.entity_id) %}{% if a %}{% set _ = o.append([s.entity_id, a]) %}{% endif %}{% endfor %}{{ o | tojson }}";

// ─── types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HaDevice {
    pub entity_id: String,
    pub name: String,
    /// "light" | "switch" | "fan" | "climate" | "cover" | "scene" | "media" | "sensor"
    pub device_type: String,
    pub room: Option<String>,
    /// None when HA says unknown / unavailable (the page shows "No reading").
    pub state: Option<String>,
    pub unit: Option<String>,
    /// Backup / container / supervisor entities: status-only, listed under System.
    pub system: bool,
    pub controllable: bool,
    /// The `service` names `ha_device_set` accepts for this device (empty when status-only).
    pub services: Vec<String>,
    pub brightness_pct: Option<f64>,
    pub percentage: Option<f64>,
    pub temperature: Option<f64>,
    pub current_temperature: Option<f64>,
    pub min_temperature: Option<f64>,
    pub max_temperature: Option<f64>,
    pub hvac_modes: Vec<String>,
    /// 0..1
    pub volume: Option<f64>,
    /// The one level service this device accepts, when it reports support for it: "turn_on" (light
    /// brightness), "set_percentage" (fan), "set_cover_position" (cover), "volume_set" (media). None otherwise.
    pub level_service: Option<String>,
    /// Current level in the units of `level_service`: 0..=100, or 0..=1 for "volume_set".
    pub level: Option<f64>,
    /// Upper bound of `level`: 100, or 1 for "volume_set".
    pub level_max: Option<f64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HaDevicesResult {
    pub devices: Vec<HaDevice>,
    /// false when the area lookup failed (rooms are then all None).
    pub rooms_available: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HaDeviceSetResult {
    /// History id: `add_undo("ha", id)` within 8 s. None when there is nothing to undo.
    pub id: Option<String>,
    pub undoable: bool,
    /// Short sentence for the toast.
    pub message: String,
}

// ─── classification ──────────────────────────────────────────────────────────

fn tokens(s: &str) -> Vec<String> {
    s.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

fn is_system(entity_id: &str, name: &str) -> bool {
    let mut t = tokens(entity_id);
    t.extend(tokens(name));
    if t.iter()
        .any(|w| matches!(w.as_str(), "backup" | "hassio" | "supervisor" | "container"))
    {
        return true;
    }
    t.windows(3).any(|w| {
        w[0] == "home" && w[1] == "assistant" && matches!(w[2].as_str(), "core" | "os" | "host")
    })
}

fn device_type(domain: &str) -> Option<&'static str> {
    Some(match domain {
        "light" => "light",
        "switch" => "switch",
        "fan" => "fan",
        "climate" => "climate",
        "cover" => "cover",
        "scene" => "scene",
        "media_player" => "media",
        "sensor" | "binary_sensor" => "sensor",
        _ => return None,
    })
}

/// Flattened, capped, with bidi / zero-width / control characters removed.
fn clean(s: &str, max: usize) -> String {
    clean_text(s, max)
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite())
}

/// Words that mark a door, gate, garage or opener (English and Japanese), case-folded substring match over the
/// entity id and the friendly name. Such a device is status-only in every domain, and a scene with one in
/// its name is refused.
const GATE_WORDS: &[&str] = &[
    "garage",
    "gate",
    "door",
    "opener",
    "車庫",
    "ガレージ",
    "門",
    "玄関",
    "扉",
];

fn is_gate_like(entity_id: &str, name: &str) -> bool {
    let t = format!("{entity_id} {name}").to_lowercase();
    GATE_WORDS.iter().any(|w| t.contains(w))
}

/// Covers are controllable only when HA says they are blinds and the like. Missing or other class: status-only.
fn cover_class_ok(attrs: &Value) -> bool {
    matches!(
        attrs["device_class"].as_str(),
        Some("blind" | "curtain" | "shade" | "shutter" | "awning" | "window")
    )
}

const SENSOR_CLASSES: &[&str] = &[
    "temperature",
    "humidity",
    "power",
    "energy",
    "battery",
    "illuminance",
    "pressure",
    "co2",
    "pm25",
    "voltage",
    "current",
];
const BINARY_CLASSES: &[&str] = &["door", "window", "motion"];

/// sensor / binary_sensor output is an allowlist of device classes; anything presence- or location-like is dropped.
fn sensor_ok(e: &ha::HaEntity) -> bool {
    let class = e.attributes["device_class"].as_str().unwrap_or("");
    let listed = if e.domain == "sensor" {
        SENSOR_CLASSES.contains(&class)
    } else {
        BINARY_CLASSES.contains(&class)
    };
    let mut t = tokens(&e.entity_id);
    t.extend(tokens(e.friendly_name.as_deref().unwrap_or("")));
    let personal = t.iter().any(|w| {
        matches!(
            w.as_str(),
            "presence"
                | "occupancy"
                | "location"
                | "tracker"
                | "person"
                | "gps"
                | "geofence"
                | "home"
                | "away"
        )
    });
    listed && !personal
}

/// HA `supported_features` bit test (a plain integer attribute).
fn feature(attrs: &Value, bit: u64) -> bool {
    attrs["supported_features"]
        .as_u64()
        .is_some_and(|f| f & bit != 0)
}

/// The level service a device accepts, judged only from the attributes HA reports. Lights need a colour mode
/// that is more than `onoff`; fans need SET_SPEED (1); covers SET_POSITION (4); media players VOLUME_SET (4).
fn level_service_for(domain: &str, attrs: &Value) -> Option<&'static str> {
    match domain {
        "light" => attrs["supported_color_modes"]
            .as_array()
            .is_some_and(|m| {
                m.iter().filter_map(Value::as_str).any(|x| {
                    matches!(
                        x,
                        "brightness"
                            | "color_temp"
                            | "hs"
                            | "xy"
                            | "rgb"
                            | "rgbw"
                            | "rgbww"
                            | "white"
                    )
                })
            })
            .then_some("turn_on"),
        "fan" => feature(attrs, 1).then_some("set_percentage"),
        "cover" => feature(attrs, 4).then_some("set_cover_position"),
        "media_player" => feature(attrs, 4).then_some("volume_set"),
        _ => None,
    }
}

/// True when `service` with `value` is a level change (not a plain on / off).
fn is_level_call(domain: &str, service: &str, value: Option<&Value>) -> bool {
    match (domain, service) {
        ("light", "turn_on") => value.is_some_and(|v| !v.is_null()),
        ("fan", "set_percentage")
        | ("cover", "set_cover_position")
        | ("media_player", "volume_set") => true,
        _ => false,
    }
}

fn services_for(domain: &str) -> &'static [&'static str] {
    match domain {
        "light" | "switch" => &["turn_on", "turn_off", "toggle"],
        "fan" => &["turn_on", "turn_off", "toggle", "set_percentage"],
        "climate" => &["set_temperature", "set_hvac_mode"],
        "cover" => &[
            "open_cover",
            "close_cover",
            "stop_cover",
            "set_cover_position",
        ],
        "scene" => &["turn_on"],
        "media_player" => &["media_play", "media_pause", "volume_set"],
        _ => &[],
    }
}

fn device_from(e: &ha::HaEntity, rooms: &HashMap<String, String>) -> Option<HaDevice> {
    let ty = device_type(&e.domain)?;
    let name = clean(
        e.friendly_name
            .as_deref()
            .unwrap_or_else(|| e.entity_id.split_once('.').map_or("", |(_, o)| o)),
        NAME_MAX,
    );
    let system = is_system(&e.entity_id, &name);
    if !system && ty == "sensor" && !sensor_ok(e) {
        return None;
    }
    let gate = is_gate_like(&e.entity_id, &name);
    let cover_ok = e.domain != "cover" || cover_class_ok(&e.attributes);
    let controllable = !system && !gate && cover_ok && !services_for(&e.domain).is_empty();
    let a = &e.attributes;
    let level = if controllable {
        level_service_for(&e.domain, a)
    } else {
        None
    };
    let (level_now, level_max) = match level {
        Some("turn_on") => (
            num(&a["brightness"]).map(|b| (b / 255.0 * 100.0).round().clamp(0.0, 100.0)),
            Some(100.0),
        ),
        Some("set_percentage") => (num(&a["percentage"]), Some(100.0)),
        Some("set_cover_position") => (num(&a["current_position"]), Some(100.0)),
        Some("volume_set") => (num(&a["volume_level"]), Some(1.0)),
        _ => (None, None),
    };
    let state = match e.state.as_str() {
        "" | "unknown" | "unavailable" => None,
        s => Some(clean(s, STATE_MAX)),
    };
    let unit = a["unit_of_measurement"]
        .as_str()
        .map(|u| clean(u, 16))
        .filter(|u| !u.is_empty());
    Some(HaDevice {
        entity_id: e.entity_id.clone(),
        name,
        device_type: ty.into(),
        room: rooms.get(&e.entity_id).cloned(),
        state,
        unit,
        system,
        controllable,
        services: if controllable {
            services_for(&e.domain)
                .iter()
                // Level services are listed only when the device reports support for them.
                .filter(|s| match **s {
                    "set_percentage" | "set_cover_position" | "volume_set" => level == Some(**s),
                    _ => true,
                })
                .map(|s| s.to_string())
                .collect()
        } else {
            Vec::new()
        },
        brightness_pct: num(&a["brightness"])
            .map(|b| (b / 255.0 * 100.0).round().clamp(0.0, 100.0)),
        percentage: num(&a["percentage"]),
        temperature: num(&a["temperature"]),
        current_temperature: num(&a["current_temperature"]),
        min_temperature: num(&a["min_temp"]),
        max_temperature: num(&a["max_temp"]),
        hvac_modes: a["hvac_modes"]
            .as_array()
            .map(|m| {
                m.iter()
                    .filter_map(Value::as_str)
                    .filter(|m| ha::HVAC_MODES.contains(m))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        volume: num(&a["volume_level"]),
        level_service: level.map(str::to_string),
        level: level_now,
        level_max,
    })
}

fn rooms_from(v: &Value) -> Option<HashMap<String, String>> {
    let list = v.as_array()?;
    Some(
        list.iter()
            .filter_map(|p| {
                let id = p.get(0)?.as_str()?;
                let area = clean(p.get(1)?.as_str()?, NAME_MAX);
                (!area.is_empty()).then(|| (id.to_string(), area))
            })
            .collect(),
    )
}

pub(super) async fn devices_with(
    cfg: &HaConfig,
    with_rooms: bool,
) -> Result<HaDevicesResult, String> {
    let states = ha::states_with(cfg).await?;
    // The write path skips the area template: it only needs the entity and its state.
    let rooms = if with_rooms {
        match ha::render_template_with(cfg, AREA_TEMPLATE).await {
            Ok(v) => rooms_from(&v),
            Err(_) => None,
        }
    } else {
        None
    };
    let rooms_available = rooms.is_some();
    let rooms = rooms.unwrap_or_default();
    Ok(HaDevicesResult {
        devices: states
            .iter()
            .filter_map(|e| device_from(e, &rooms))
            .collect(),
        rooms_available,
    })
}

// ─── the write allowlist ─────────────────────────────────────────────────────

/// `^[a-z_]+\.[a-z0-9_]+$`
fn entity_id_ok(s: &str) -> bool {
    match s.split_once('.') {
        Some((d, o)) => {
            !d.is_empty()
                && d.len() <= 64
                && d.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
                && !o.is_empty()
                && o.len() <= 128
                && o.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        }
        None => false,
    }
}

/// Sample controls (the Smart home page's example devices) use ids with these object prefixes; they are never
/// sent live.
fn is_sample_id(entity_id: &str) -> bool {
    entity_id
        .split_once('.')
        .is_some_and(|(_, o)| o.starts_with("sample_") || o.starts_with("example_"))
}

/// A validated HA call: the HA service name and the extra body fields.
struct Call {
    ha_service: &'static str,
    extra: Map<String, Value>,
}

/// A whole number in `lo..=hi` (JSON number; a string, bool or fraction is refused).
fn whole(value: Option<&Value>, lo: f64, hi: f64) -> Result<f64, String> {
    value
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite() && n.fract() == 0.0 && (lo..=hi).contains(n))
        .ok_or_else(|| ha::ERR_BAD_DATA.to_string())
}

/// Domain, service and value to a call, or a refusal. The one allowlist: used for the write and for Undo.
fn plan(
    domain: &str,
    service: &str,
    value: Option<&Value>,
    min_max: Option<(f64, f64)>,
) -> Result<Call, String> {
    let none = |s: &'static str| -> Result<Call, String> {
        if value.is_some_and(|v| !v.is_null()) {
            return Err(ha::ERR_BAD_DATA.to_string());
        }
        Ok(Call {
            ha_service: s,
            extra: Map::new(),
        })
    };
    let one = |s: &'static str, k: &str, v: Value| {
        let mut extra = Map::new();
        extra.insert(k.into(), v);
        Ok(Call {
            ha_service: s,
            extra,
        })
    };
    if !service.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
        || service.is_empty()
        || service.len() > 64
    {
        return Err(ha::ERR_INVALID.to_string());
    }
    match (domain, service) {
        ("light", "turn_on") => match value.filter(|v| !v.is_null()) {
            None => none("turn_on"),
            Some(_) => one(
                "turn_on",
                "brightness_pct",
                json!(whole(value, 1.0, 100.0)?),
            ),
        },
        ("light" | "switch" | "fan", "turn_off") => none("turn_off"),
        ("switch" | "fan", "turn_on") => none("turn_on"),
        ("light" | "switch" | "fan", "toggle") => none("toggle"),
        ("fan", "set_percentage") => one(
            "set_percentage",
            "percentage",
            json!(whole(value, 0.0, 100.0)?),
        ),
        ("cover", "set_cover_position") => one(
            "set_cover_position",
            "position",
            json!(whole(value, 0.0, 100.0)?),
        ),
        ("climate", "set_temperature") => {
            let (lo, hi) = min_max
                .filter(|(a, b)| a.is_finite() && b.is_finite() && a < b)
                .map(|(a, b)| (a.max(ha::MIN_TEMPERATURE), b.min(ha::MAX_TEMPERATURE)))
                .unwrap_or((ha::MIN_TEMPERATURE, ha::MAX_TEMPERATURE));
            let t = value
                .and_then(Value::as_f64)
                .filter(|t| t.is_finite() && (lo..=hi).contains(t))
                .ok_or_else(|| ha::ERR_BAD_DATA.to_string())?;
            one("set_temperature", "temperature", json!(t))
        }
        ("climate", "set_hvac_mode") => {
            let m = value
                .and_then(Value::as_str)
                .filter(|m| ha::HVAC_MODES.contains(m))
                .ok_or_else(|| ha::ERR_BAD_DATA.to_string())?;
            one("set_hvac_mode", "hvac_mode", json!(m))
        }
        ("cover", "open_cover") => none("open_cover"),
        ("cover", "close_cover") => none("close_cover"),
        ("cover", "stop_cover") => none("stop_cover"),
        ("scene", "turn_on") => none("turn_on"),
        ("media_player", "media_play") => none("media_play"),
        ("media_player", "media_pause") => none("media_pause"),
        ("media_player", "volume_set") => {
            let v = value
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))
                .ok_or_else(|| ha::ERR_BAD_DATA.to_string())?;
            one("volume_set", "volume_level", json!(v))
        }
        // Wrong service for a controllable domain: not on the list.
        _ => Err(ha::ERR_NOT_ALLOWED.to_string()),
    }
}

/// What to say in History.
fn action_text(service: &str, value: Option<&Value>) -> String {
    let n = value.and_then(Value::as_f64);
    match (service, n) {
        ("turn_on", Some(p)) => format!("set to {}%", p.round()),
        ("turn_on", None) => "turned on".into(),
        ("turn_off", _) => "turned off".into(),
        ("toggle", _) => "toggled".into(),
        ("set_percentage", Some(p)) => format!("speed {}%", p.round()),
        ("set_cover_position", Some(p)) => format!("position {}%", p.round()),
        ("set_temperature", Some(t)) => format!("set to {t}"),
        ("set_hvac_mode", _) => format!("mode {}", value.and_then(Value::as_str).unwrap_or("")),
        ("open_cover", _) => "opened".into(),
        ("close_cover", _) => "closed".into(),
        ("stop_cover", _) => "stopped".into(),
        ("media_play", _) => "playing".into(),
        ("media_pause", _) => "paused".into(),
        ("volume_set", Some(v)) => format!("volume {}%", (v * 100.0).round()),
        _ => "changed".into(),
    }
}

/// The call that puts a device back as it was, or None when the change can't be taken back.
fn prior_call(domain: &str, service: &str, d: &HaDevice) -> Option<(String, Option<Value>)> {
    let on = d.state.as_deref() == Some("on");
    let off = d.state.as_deref() == Some("off");
    match (domain, service) {
        ("light", "turn_on" | "turn_off" | "toggle") => {
            if on {
                // A brightness restore only where the device accepts a level; else a plain turn_on.
                let level = d.level_service.as_deref() == Some("turn_on");
                Some((
                    "turn_on".into(),
                    d.brightness_pct
                        .filter(|_| level)
                        .map(|b| json!(b.round().max(1.0))),
                ))
            } else if off {
                Some(("turn_off".into(), None))
            } else {
                None
            }
        }
        ("switch", "turn_on" | "turn_off" | "toggle") => {
            if on {
                Some(("turn_on".into(), None))
            } else if off {
                Some(("turn_off".into(), None))
            } else {
                None
            }
        }
        ("fan", "turn_on" | "turn_off" | "toggle" | "set_percentage") => {
            if on {
                match d
                    .percentage
                    .filter(|_| d.level_service.as_deref() == Some("set_percentage"))
                {
                    Some(p) => Some((
                        "set_percentage".into(),
                        Some(json!(p.round().clamp(0.0, 100.0))),
                    )),
                    None => Some(("turn_on".into(), None)),
                }
            } else if off {
                Some(("turn_off".into(), None))
            } else {
                None
            }
        }
        ("climate", "set_temperature") => d
            .temperature
            .map(|t| ("set_temperature".to_string(), Some(json!(t)))),
        ("climate", "set_hvac_mode") => d
            .state
            .as_deref()
            .filter(|m| ha::HVAC_MODES.contains(m))
            .map(|m| ("set_hvac_mode".to_string(), Some(json!(m)))),
        ("cover", "open_cover" | "close_cover" | "set_cover_position") => {
            match d.state.as_deref() {
                Some("open") => Some(("open_cover".into(), None)),
                Some("closed") => Some(("close_cover".into(), None)),
                _ => None,
            }
        }
        ("media_player", "media_play" | "media_pause") => match d.state.as_deref() {
            Some("playing") => Some(("media_play".into(), None)),
            Some("paused") => Some(("media_pause".into(), None)),
            _ => None,
        },
        ("media_player", "volume_set") => {
            d.volume.map(|v| ("volume_set".to_string(), Some(json!(v))))
        }
        _ => None,
    }
}

// ─── running things ──────────────────────────────────────────────────────────

/// Runs a future to completion on a private thread and runtime. `logged_write` and Undo's `revert` are
/// synchronous and called from blocking threads (and plain tests), so the HA call is driven here.
fn run_sync<F>(f: F) -> F::Output
where
    F: std::future::Future + Send,
    F::Output: Send,
{
    std::thread::scope(|s| {
        s.spawn(|| {
            match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt.block_on(f),
                Err(_) => unreachable!("runtime build"),
            }
        })
        .join()
        .unwrap_or_else(|_| panic!("ha call thread"))
    })
}

#[cfg(test)]
static TEST_CFG: std::sync::Mutex<Option<HaConfig>> = std::sync::Mutex::new(None);

fn live_config() -> Result<HaConfig, String> {
    #[cfg(test)]
    if let Some(c) = TEST_CFG.lock().unwrap_or_else(|p| p.into_inner()).clone() {
        return Ok(c);
    }
    ha::load_config()
}

fn body_for(entity_id: &str, call: &Call) -> Value {
    let mut b = call.extra.clone();
    b.insert("entity_id".into(), Value::String(entity_id.into()));
    Value::Object(b)
}

async fn fast<T>(f: impl std::future::Future<Output = Result<T, String>>) -> Result<T, String> {
    tokio::time::timeout(REQ_TIMEOUT, f)
        .await
        .unwrap_or_else(|_| Err(ERR_TIMEOUT.to_string()))
}

/// Undo of an "ha" History entry: re-checks the device (listed, controllable, not system, not a gate) and
/// re-applies the stored prior state through the same allowlist. Called by `adds::revert`.
pub(crate) fn restore_blocking(
    entity_id: &str,
    restore: Option<&(String, Option<Value>)>,
) -> Result<(), String> {
    let Some((service, value)) = restore else {
        return Err("ha: this change can't be undone".into());
    };
    if !entity_id_ok(entity_id) || is_sample_id(entity_id) {
        return Err(ha::ERR_INVALID.to_string());
    }
    let domain = entity_id.split('.').next().unwrap_or("");
    // Covers need a native approval and never get an Undo (see set_with).
    if domain == "cover" {
        return Err(ERR_REFUSED.to_string());
    }
    if services_for(domain).is_empty() {
        return Err(ha::ERR_NOT_ALLOWED.to_string());
    }
    let call = plan(domain, service, value.as_ref(), None)?;
    let cfg = live_config()?;
    let body = body_for(entity_id, &call);
    run_sync(async move {
        let list = fast(devices_with(&cfg, false)).await?;
        let ok = list.devices.iter().any(|d| {
            d.entity_id == entity_id
                && d.controllable
                && !d.system
                && (!is_level_call(domain, service, value.as_ref())
                    || d.level_service.as_deref() == Some(service))
        });
        if !ok {
            return Err(ERR_STATUS_ONLY.to_string());
        }
        fast(ha::call_service_with(&cfg, domain, call.ha_service, &body)).await
    })
}

/// Holds the native approval for the length of the write.
pub(super) type Guard = Box<dyn std::any::Any + Send>;

/// Both HA calls (state read, service call) run here with no History lock held; the lock is taken only to
/// write the History line afterwards (`adds::logged_record`). `approve(name)` is asked for scenes only.
pub(super) async fn set_with<A, Fut>(
    cfg: HaConfig,
    dirs: Dirs,
    approve: A,
    entity_id: String,
    service: String,
    value: Option<Value>,
    via: Option<String>,
) -> Result<HaDeviceSetResult, String>
where
    A: Fn(String, String) -> Fut,
    Fut: std::future::Future<Output = Result<Guard, String>>,
{
    if !entity_id_ok(&entity_id) {
        return Err(ha::ERR_INVALID.to_string());
    }
    if is_sample_id(&entity_id) {
        return Err(ERR_SAMPLE.to_string());
    }
    let domain = entity_id.split('.').next().unwrap_or("").to_string();
    if services_for(&domain).is_empty() {
        return Err(ERR_REFUSED.to_string());
    }
    // Shape check first (no network). The setpoint range is tightened below once the device is known.
    plan(&domain, &service, value.as_ref(), None)?;
    let list = fast(devices_with(&cfg, false)).await?;
    let dev = list
        .devices
        .iter()
        .find(|d| d.entity_id == entity_id)
        .ok_or_else(|| ERR_UNKNOWN.to_string())?;
    if dev.system || !dev.controllable {
        return Err(ERR_STATUS_ONLY.to_string());
    }
    // A level change needs the device to report support for that level service.
    if is_level_call(&domain, &service, value.as_ref())
        && dev.level_service.as_deref() != Some(service.as_str())
    {
        return Err(ha::ERR_NOT_ALLOWED.to_string());
    }
    let min_max = dev.min_temperature.zip(dev.max_temperature);
    let call = plan(&domain, &service, value.as_ref(), min_max)?;
    // Covers need a native approval (parity with ha_call_service's `ha:cover` rule), so they get no
    // Undo: an Undo would move the cover again without asking.
    let restore = if domain == "cover" {
        None
    } else {
        prior_call(&domain, &service, dev)
    };
    let text = format!("{}: {}", dev.name, action_text(&service, value.as_ref()));
    let undoable = restore.is_some();
    let body = body_for(&entity_id, &call);
    let undo = UndoRef::Ha {
        entity_id: entity_id.clone(),
        restore: restore.map(|(service, value)| json!({ "service": service, "value": value })),
    };
    let is_scene = domain == "scene";
    // A scene can change many devices at once and can't be undone; a cover moves something physical
    // (same rule as ha_call_service): a native dialog each time for both.
    let _guard = if is_scene || domain == "cover" {
        Some(approve(domain.clone(), dev.name.clone()).await?)
    } else {
        None
    };
    fast(ha::call_service_with(&cfg, &domain, call.ha_service, &body)).await?;
    // Taken after the HA call so the 8 s Undo window starts when the device actually changed.
    let now = now_jst();
    let hist = blocking(JoinErr::Plain, move || {
        adds::logged_record("ha", text, via.as_deref(), &dirs, now, undo)
    })
    .await
    .map_err(|_| ERR_NOT_RECORDED.to_string())?;
    Ok(HaDeviceSetResult {
        id: undoable.then_some(hist),
        undoable,
        message: if undoable {
            "Done. Undo is available for a few seconds.".into()
        } else if is_scene {
            "Scene activated. Scenes can't be undone.".into()
        } else {
            "Done. This one can't be undone.".into()
        },
    })
}

/// Decodes the stored restore value (service + value).
pub(crate) fn decode_restore(v: &Option<Value>) -> Option<(String, Option<Value>)> {
    let v = v.as_ref()?;
    let service = v.get("service")?.as_str()?.to_string();
    let value = v.get("value").filter(|x| !x.is_null()).cloned();
    Some((service, value))
}

// ─── commands ────────────────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
pub async fn ha_devices_list() -> Result<HaDevicesResult, String> {
    devices_with(&ha::load_config()?, true).await
}

#[tauri::command]
#[specta::specta]
pub async fn ha_device_set(
    app: tauri::AppHandle,
    entity_id: String,
    service: String,
    value: Option<Value>,
    via: Option<String>,
) -> Result<HaDeviceSetResult, String> {
    // Cheap refusals before touching the config, so they never depend on HA being set up.
    if !entity_id_ok(&entity_id) {
        return Err(ha::ERR_INVALID.to_string());
    }
    if is_sample_id(&entity_id) {
        return Err(ERR_SAMPLE.to_string());
    }
    let cfg = ha::load_config()?;
    let approve = move |domain: String, name: String| {
        let app = app.clone();
        async move {
            let (title, body) = if domain == "cover" {
                (
                    "Home Assistant: move a blind or cover",
                    format!(
                        "Device: {name}\n\nThis moves something physical and can't be undone here."
                    ),
                )
            } else {
                (
                    "Home Assistant: activate a scene",
                    format!("Scene: {name}\n\nA scene can change several devices at once and can't be undone."),
                )
            };
            let text = super::approval_gate::approval_text(title, &body)
                .map_err(|_| ha::ERR_NEEDS_APPROVAL.to_string())?;
            // Same keys as ha_call_service (`ha:<domain>`), so both paths share one rule.
            let key = super::approval_gate::service_key(&format!("ha:{domain}"));
            let g = super::approval_gate::require_approval(&app, &[key], text)
                .await
                .map_err(|e| ha::approval_error(&e))?;
            Ok(Box::new(g) as Guard)
        }
    };
    set_with(cfg, Dirs::real(), approve, entity_id, service, value, via).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::local_store::tests::temp_dir;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const TOKEN: &str = "TEST-HA-TOKEN-b3-must-never-leak-0123456789";
    /// Tests that touch the shared TEST_CFG run one at a time.
    static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    type Seen = Arc<Mutex<Vec<(String, String)>>>;

    /// Fake HA on an ephemeral loopback port. Answers by path; records (path, body). Errors echo the token.
    async fn mock(states: Value, areas: Option<Value>) -> (String, Seen) {
        mock_slow(states, areas, 0).await
    }

    /// `delay_ms` slows the service-call path only.
    async fn mock_slow(states: Value, areas: Option<Value>, delay_ms: u64) -> (String, Seen) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen: Seen = Default::default();
        let s2 = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let seen = s2.clone();
                let states = states.clone();
                let areas = areas.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 65536];
                    let mut n = 0;
                    loop {
                        let r = sock.read(&mut buf[n..]).await.unwrap_or(0);
                        if r == 0 {
                            break;
                        }
                        n += r;
                        let text = String::from_utf8_lossy(&buf[..n]).to_string();
                        if let Some(i) = text.find("\r\n\r\n") {
                            let len = text
                                .lines()
                                .find_map(|l| {
                                    l.to_ascii_lowercase()
                                        .strip_prefix("content-length:")
                                        .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                                })
                                .unwrap_or(0);
                            if n >= i + 4 + len {
                                break;
                            }
                        }
                    }
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let body = req.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
                    seen.lock().unwrap().push((path.clone(), body));
                    let (status, out) = if path == "/api/states" {
                        (200, states.to_string())
                    } else if path == "/api/template" {
                        match &areas {
                            Some(a) => (200, a.to_string()),
                            None => (500, format!("{{\"message\":\"{TOKEN}\"}}")),
                        }
                    } else if path.starts_with("/api/services/") {
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        (200, "[]".to_string())
                    } else {
                        (404, format!("{{\"message\":\"{TOKEN}\"}}"))
                    };
                    let resp = format!(
                        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{out}",
                        out.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                });
            }
        });
        (format!("http://{addr}"), seen)
    }

    fn st(id: &str, state: &str, attrs: Value) -> Value {
        json!({"entity_id": id, "state": state, "attributes": attrs})
    }

    fn fixture() -> Value {
        json!([
            st(
                "light.kitchen",
                "on",
                json!({"friendly_name": "Kitchen", "brightness": 128,
                "supported_color_modes": ["brightness"],
                "entity_picture": format!("/x?token={TOKEN}"), "access_token": TOKEN})
            ),
            st("switch.heater", "off", json!({"friendly_name": "Heater"})),
            st(
                "light.plain",
                "on",
                json!({"friendly_name": "Plain", "brightness": 255, "supported_color_modes": ["onoff"]})
            ),
            st(
                "fan.basic",
                "on",
                json!({"friendly_name": "Basic fan", "supported_features": 0})
            ),
            st(
                "cover.nopos",
                "open",
                json!({"friendly_name": "Slat", "device_class": "shade", "supported_features": 3})
            ),
            st(
                "media_player.mute",
                "playing",
                json!({"friendly_name": "Mute box", "supported_features": 0, "volume_level": 0.5})
            ),
            st(
                "fan.bed",
                "on",
                json!({"friendly_name": "Bed fan", "percentage": 40, "supported_features": 1})
            ),
            st(
                "climate.main",
                "heat",
                json!({"friendly_name": "Main", "temperature": 21.0,
                "current_temperature": 19.5, "min_temp": 10, "max_temp": 30,
                "hvac_modes": ["off", "heat", "weird"]})
            ),
            st(
                "cover.blind",
                "open",
                json!({"friendly_name": "Blind", "device_class": "blind",
                "supported_features": 15, "current_position": 60})
            ),
            st(
                "cover.noclass",
                "open",
                json!({"friendly_name": "Hall thing"})
            ),
            st(
                "switch.garage_opener",
                "off",
                json!({"friendly_name": "Opener"})
            ),
            st(
                "cover.garagedoor",
                "closed",
                json!({"friendly_name": "Thing", "device_class": "blind"})
            ),
            st(
                "light.porch",
                "off",
                json!({"friendly_name": "GarageDoor light"})
            ),
            st(
                "switch.genkan",
                "off",
                json!({"friendly_name": "玄関の灯り"})
            ),
            st("fan.shako", "off", json!({"friendly_name": "車庫ファン"})),
            st("light.garden", "off", json!({"friendly_name": "ガレージ"})),
            st("switch.mon", "off", json!({"friendly_name": "門灯"})),
            st("switch.shoji", "off", json!({"friendly_name": "扉"})),
            st(
                "scene.front_door_open",
                "unknown",
                json!({"friendly_name": "Welcome"})
            ),
            st("sensor.noclass", "5", json!({"friendly_name": "Mystery"})),
            st(
                "binary_sensor.presence_hall",
                "on",
                json!({"friendly_name": "Hall", "device_class": "motion"})
            ),
            st(
                "device_tracker.phone",
                "home",
                json!({"friendly_name": "Phone"})
            ),
            st(
                "cover.garage_door",
                "closed",
                json!({"friendly_name": "Garage door", "device_class": "garage"})
            ),
            st(
                "scene.evening",
                "unknown",
                json!({"friendly_name": "Evening"})
            ),
            st(
                "media_player.tv",
                "playing",
                json!({"friendly_name": "TV", "volume_level": 0.3, "supported_features": 4,
                "entity_picture": "/api/media_player_proxy/x?token=abc"})
            ),
            st(
                "sensor.temp",
                "unavailable",
                json!({"friendly_name": "Hall temp", "unit_of_measurement": "C", "device_class": "temperature"})
            ),
            st(
                "sensor.backup_manager",
                "idle",
                json!({"friendly_name": "Backup Backup manager"})
            ),
            st("lock.front", "locked", json!({"friendly_name": "Front"})),
            st(
                "camera.door",
                "idle",
                json!({"friendly_name": "Door", "access_token": TOKEN})
            ),
            st("automation.x", "on", json!({})),
        ])
    }

    fn areas() -> Value {
        json!([
            ["light.kitchen", "Kitchen"],
            ["climate.main", "Living room"]
        ])
    }

    fn dirs(tag: &str) -> Dirs {
        let d = temp_dir(tag);
        Dirs {
            data: d.join("data"),
            workspace: d.join("ws"),
            legacy: d.join("legacy"),
        }
    }

    fn cfg(base: &str) -> HaConfig {
        ha::test_config(base, TOKEN)
    }

    fn calls(seen: &Seen) -> Vec<(String, String)> {
        seen.lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p.starts_with("/api/services/"))
            .cloned()
            .collect()
    }

    fn ok_approver(
        asked: Arc<Mutex<Vec<String>>>,
    ) -> impl Fn(String, String) -> std::future::Ready<Result<Guard, String>> {
        move |_domain, name| {
            asked.lock().unwrap().push(name);
            std::future::ready(Ok(Box::new(()) as Guard))
        }
    }

    async fn set(
        base: &str,
        d: &Dirs,
        id: &str,
        service: &str,
        value: Option<Value>,
    ) -> Result<HaDeviceSetResult, String> {
        set_with(
            cfg(base),
            d.clone(),
            ok_approver(Default::default()),
            id.into(),
            service.into(),
            value,
            None,
        )
        .await
    }

    #[tokio::test]
    async fn list_maps_types_rooms_and_filters() {
        let (base, _) = mock(fixture(), Some(areas())).await;
        let r = devices_with(&cfg(&base), true).await.unwrap();
        assert!(r.rooms_available);
        let ids: Vec<&str> = r.devices.iter().map(|d| d.entity_id.as_str()).collect();
        // lock, camera, automation are not listed.
        assert!(!ids.iter().any(|i| i.starts_with("lock.")
            || i.starts_with("camera.")
            || i.starts_with("automation.")));
        let k = r
            .devices
            .iter()
            .find(|d| d.entity_id == "light.kitchen")
            .unwrap();
        assert_eq!(
            (
                k.device_type.as_str(),
                k.room.as_deref(),
                k.state.as_deref()
            ),
            ("light", Some("Kitchen"), Some("on"))
        );
        assert_eq!(k.brightness_pct, Some(50.0));
        assert!(k.controllable && k.services.contains(&"turn_on".to_string()));
        let c = r
            .devices
            .iter()
            .find(|d| d.entity_id == "climate.main")
            .unwrap();
        assert_eq!(c.room.as_deref(), Some("Living room"));
        assert_eq!(c.hvac_modes, vec!["off", "heat"]);
        assert_eq!(c.temperature, Some(21.0));
        let tv = r
            .devices
            .iter()
            .find(|d| d.entity_id == "media_player.tv")
            .unwrap();
        assert_eq!((tv.device_type.as_str(), tv.volume), ("media", Some(0.3)));
        let s = r
            .devices
            .iter()
            .find(|d| d.entity_id == "sensor.temp")
            .unwrap();
        assert_eq!(
            (s.state.clone(), s.unit.as_deref(), s.controllable),
            (None, Some("C"), false)
        );
        // Backup sensor: system, status-only.
        let b = r
            .devices
            .iter()
            .find(|d| d.entity_id == "sensor.backup_manager")
            .unwrap();
        assert!(b.system && !b.controllable && b.services.is_empty());
        // Garage cover: listed, status-only.
        let g = r
            .devices
            .iter()
            .find(|d| d.entity_id == "cover.garage_door")
            .unwrap();
        assert!(!g.controllable && g.services.is_empty());
        // Unknown scene state is "No reading".
        assert_eq!(
            r.devices
                .iter()
                .find(|d| d.entity_id == "scene.evening")
                .unwrap()
                .state,
            None
        );
    }

    #[tokio::test]
    async fn list_never_carries_tokens_or_pictures() {
        let (base, _) = mock(fixture(), Some(areas())).await;
        let r = devices_with(&cfg(&base), true).await.unwrap();
        let text = format!("{r:?}");
        assert!(
            !text.contains(TOKEN) && !text.contains("token=") && !text.contains("entity_picture")
        );
        assert!(!serde_json::to_string(&r).unwrap().contains("access_token"));
    }

    #[tokio::test]
    async fn rooms_fall_back_when_the_area_lookup_fails() {
        let (base, _) = mock(fixture(), None).await;
        let r = devices_with(&cfg(&base), true).await.unwrap();
        assert!(!r.rooms_available);
        assert!(r.devices.iter().all(|d| d.room.is_none()));
        assert!(!r.devices.is_empty());
    }

    #[tokio::test]
    async fn refusals_never_reach_ha() {
        let (base, seen) = mock(fixture(), Some(areas())).await;
        let d = dirs("ha-refuse");
        for (id, svc, val) in [
            ("lock.front", "unlock", None),
            ("lock.front", "lock", None),
            ("alarm_control_panel.home", "alarm_disarm", None),
            ("camera.door", "turn_on", None),
            ("script.go", "turn_on", None),
            ("automation.x", "trigger", None),
            ("shell_command.x", "run", None),
            ("rest_command.x", "run", None),
            ("homeassistant.restart", "turn_off", None),
            ("homeassistant.restart", "restart", None),
            ("light.kitchen", "homeassistant.restart", None),
            ("light.kitchen", "reload", None),
            ("light.kitchen", "turn_off", Some(json!(1))),
            ("BAD ID", "turn_on", None),
            ("light", "turn_on", None),
            ("light.Kitchen", "turn_on", None),
        ] {
            let e = set(&base, &d, id, svc, val).await.unwrap_err();
            assert!(
                e.starts_with("not_allowed:") || e.starts_with("invalid_input:"),
                "{id} {svc}: {e}"
            );
            assert!(!e.contains(TOKEN));
        }
        assert!(calls(&seen).is_empty());
    }

    #[tokio::test]
    async fn unknown_system_and_gate_devices_are_refused() {
        let (base, seen) = mock(fixture(), Some(areas())).await;
        let d = dirs("ha-unknown");
        let e = set(&base, &d, "light.nope", "turn_on", None)
            .await
            .unwrap_err();
        assert!(e.starts_with("invalid_input:"));
        let e = set(&base, &d, "cover.garage_door", "open_cover", None)
            .await
            .unwrap_err();
        assert!(e.starts_with("not_allowed:"));
        let e = set(&base, &d, "sensor.temp", "turn_on", None)
            .await
            .unwrap_err();
        assert!(e.starts_with("not_allowed:"));
        assert!(calls(&seen).is_empty());
    }

    #[tokio::test]
    async fn sample_ids_are_refused_before_any_call() {
        let (base, seen) = mock(fixture(), Some(areas())).await;
        let d = dirs("ha-sample");
        for id in [
            "light.sample_living",
            "climate.example_main",
            "scene.sample_evening",
        ] {
            let e = set(&base, &d, id, "turn_on", None).await.unwrap_err();
            assert!(e.contains("Sample"), "{e}");
        }
        assert!(seen.lock().unwrap().is_empty());
        let r = restore_blocking("light.sample_living", Some(&("turn_off".into(), None)));
        assert!(r.is_err());
    }

    #[test]
    fn value_ranges() {
        let v = |x: Value| Some(x);
        assert!(plan("light", "turn_on", v(json!(0)).as_ref(), None).is_err());
        assert!(plan("light", "turn_on", v(json!(1)).as_ref(), None).is_ok());
        assert!(plan("light", "turn_on", v(json!(100)).as_ref(), None).is_ok());
        assert!(plan("light", "turn_on", v(json!(101)).as_ref(), None).is_err());
        assert!(plan("light", "turn_on", v(json!(-1)).as_ref(), None).is_err());
        assert!(plan("light", "turn_on", v(json!("50")).as_ref(), None).is_err());
        assert!(plan("fan", "set_percentage", None, None).is_err());
        assert!(plan("fan", "set_percentage", v(json!(100)).as_ref(), None).is_ok());
        assert!(plan("climate", "set_temperature", v(json!(4)).as_ref(), None).is_err());
        assert!(plan("climate", "set_temperature", v(json!(96)).as_ref(), None).is_err());
        assert!(plan("climate", "set_temperature", v(json!(21.5)).as_ref(), None).is_ok());
        // Device limits tighten the range.
        assert!(plan(
            "climate",
            "set_temperature",
            v(json!(35)).as_ref(),
            Some((10.0, 30.0))
        )
        .is_err());
        assert!(plan("climate", "set_hvac_mode", v(json!("heat")).as_ref(), None).is_ok());
        assert!(plan("climate", "set_hvac_mode", v(json!("lava")).as_ref(), None).is_err());
        assert!(plan("media_player", "volume_set", v(json!(1.0)).as_ref(), None).is_ok());
        assert!(plan("media_player", "volume_set", v(json!(1.1)).as_ref(), None).is_err());
        assert!(plan("media_player", "volume_set", v(json!(-0.1)).as_ref(), None).is_err());
        assert!(plan("switch", "toggle", v(json!(1)).as_ref(), None).is_err());
        assert!(plan("scene", "turn_on", None, None).is_ok());
        assert!(plan("lock", "unlock", None, None).is_err());
    }

    #[tokio::test]
    async fn a_good_write_calls_ha_logs_history_and_has_no_secret() {
        let (base, seen) = mock(fixture(), Some(areas())).await;
        let d = dirs("ha-write");
        let r = set(&base, &d, "light.kitchen", "turn_on", Some(json!(30)))
            .await
            .unwrap();
        assert!(r.undoable && r.id.is_some());
        let c = calls(&seen);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].0, "/api/services/light/turn_on");
        let body: Value = serde_json::from_str(&c[0].1).unwrap();
        assert_eq!(
            body,
            json!({"entity_id": "light.kitchen", "brightness_pct": 30.0})
        );
        let h = adds::history_in("ha", &d).unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].text, "Kitchen: set to 30%");
        let raw = std::fs::read_to_string(d.data.join("adds").join("history.json")).unwrap();
        assert!(!raw.contains(TOKEN) && !raw.contains("token="));
    }

    #[tokio::test]
    async fn undo_reapplies_the_prior_state_within_the_window() {
        let _g = SERIAL.lock().await;
        let (base, seen) = mock(fixture(), Some(areas())).await;
        *TEST_CFG.lock().unwrap() = Some(cfg(&base));
        let d = dirs("ha-undo");
        // Kitchen was on at 50%; turning it off then undoing puts it back on at 50%.
        let r = set(&base, &d, "light.kitchen", "turn_off", None)
            .await
            .unwrap();
        let id = r.id.unwrap();
        let d2 = d.clone();
        let ok = tokio::task::spawn_blocking(move || adds::undo_in("ha", &id, &d2, now_jst()))
            .await
            .unwrap()
            .unwrap();
        assert!(ok);
        let c = calls(&seen);
        assert_eq!(c.len(), 2);
        assert_eq!(c[1].0, "/api/services/light/turn_on");
        let body: Value = serde_json::from_str(&c[1].1).unwrap();
        assert_eq!(
            body,
            json!({"entity_id": "light.kitchen", "brightness_pct": 50.0})
        );
        assert!(adds::history_in("ha", &d).unwrap().is_empty());

        // Climate setpoint goes back to 21.
        let r = set(
            &base,
            &d,
            "climate.main",
            "set_temperature",
            Some(json!(24)),
        )
        .await
        .unwrap();
        let id = r.id.unwrap();
        let d3 = d.clone();
        tokio::task::spawn_blocking(move || adds::undo_in("ha", &id, &d3, now_jst()))
            .await
            .unwrap()
            .unwrap();
        let last = calls(&seen).pop().unwrap();
        assert_eq!(last.0, "/api/services/climate/set_temperature");
        assert_eq!(
            serde_json::from_str::<Value>(&last.1).unwrap()["temperature"],
            json!(21.0)
        );
        *TEST_CFG.lock().unwrap() = None;
    }

    #[tokio::test]
    async fn undo_is_refused_after_eight_seconds() {
        let _g = SERIAL.lock().await;
        let (base, seen) = mock(fixture(), Some(areas())).await;
        *TEST_CFG.lock().unwrap() = Some(cfg(&base));
        let d = dirs("ha-late");
        let r = set(&base, &d, "switch.heater", "turn_on", None)
            .await
            .unwrap();
        let id = r.id.unwrap();
        let late = now_jst() + chrono::Duration::seconds(9);
        let d2 = d.clone();
        let ok = tokio::task::spawn_blocking(move || adds::undo_in("ha", &id, &d2, late))
            .await
            .unwrap()
            .unwrap();
        assert!(!ok);
        assert_eq!(calls(&seen).len(), 1);
        *TEST_CFG.lock().unwrap() = None;
    }

    #[tokio::test]
    async fn scenes_have_no_undo_and_say_so() {
        let (base, _) = mock(fixture(), Some(areas())).await;
        let d = dirs("ha-scene");
        let r = set(&base, &d, "scene.evening", "turn_on", None)
            .await
            .unwrap();
        assert!(!r.undoable && r.id.is_none());
        assert!(r.message.contains("can't be undone"));
        // History still has the line; its Undo refuses.
        let h = adds::history_in("ha", &d).unwrap();
        assert_eq!(h.len(), 1);
        assert!(adds::undo_in("ha", &h[0].id, &d, now_jst()).is_err());
    }

    #[tokio::test]
    async fn failed_ha_call_leaves_no_history_and_no_token() {
        // 404 on the service path: the mock echoes the token in its body.
        let (base, _) = mock(
            json!([st(
                "light.kitchen",
                "on",
                json!({"friendly_name": "Kitchen"})
            )]),
            None,
        )
        .await;
        let d = dirs("ha-fail");
        let dead = cfg("http://127.0.0.1:9");
        let e = set_with(
            dead,
            d.clone(),
            ok_approver(Default::default()),
            "light.kitchen".into(),
            "turn_off".into(),
            None,
            None,
        )
        .await
        .unwrap_err();
        assert!(e.starts_with("unreachable:") && !e.contains(TOKEN));
        assert!(adds::history_in("ha", &d).unwrap().is_empty());
        let _ = base;
    }

    #[tokio::test]
    async fn gate_words_make_any_domain_status_only() {
        let (base, seen) = mock(fixture(), Some(areas())).await;
        let r = devices_with(&cfg(&base), true).await.unwrap();
        let d = dirs("ha-gate");
        for id in [
            "switch.garage_opener",
            "cover.garagedoor",
            "light.porch",
            "switch.genkan",
            "fan.shako",
            "light.garden",
            "switch.mon",
            "switch.shoji",
            "cover.noclass",
            "scene.front_door_open",
            "cover.garage_door",
        ] {
            let dev = r.devices.iter().find(|x| x.entity_id == id).unwrap();
            assert!(!dev.controllable && dev.services.is_empty(), "{id}");
            let svc = if id.starts_with("cover.") {
                "open_cover"
            } else {
                "turn_on"
            };
            let e = set(&base, &d, id, svc, None).await.unwrap_err();
            assert!(e.starts_with("not_allowed:"), "{id}: {e}");
        }
        let blind = r
            .devices
            .iter()
            .find(|x| x.entity_id == "cover.blind")
            .unwrap();
        assert!(blind.controllable);
        assert!(calls(&seen).is_empty());
    }

    #[tokio::test]
    async fn sensors_are_an_allowlist_and_presence_is_dropped() {
        let (base, _) = mock(fixture(), Some(areas())).await;
        let r = devices_with(&cfg(&base), true).await.unwrap();
        let ids: Vec<&str> = r.devices.iter().map(|d| d.entity_id.as_str()).collect();
        assert!(ids.contains(&"sensor.temp"));
        assert!(ids.contains(&"sensor.backup_manager"));
        for gone in [
            "sensor.noclass",
            "binary_sensor.presence_hall",
            "device_tracker.phone",
        ] {
            assert!(!ids.contains(&gone), "{gone}");
        }
    }

    #[test]
    fn names_lose_bidi_and_zero_width_characters() {
        assert_eq!(clean("Ki\u{202e}tc\u{200b}hen\u{2066}", 80), "Kitchen");
    }

    #[tokio::test]
    async fn a_cover_needs_a_native_approval_and_has_no_undo() {
        // Parity with ha_call_service (`ha:cover` needs approval): every blind move asks, and a
        // declined one sends nothing; an Undo can't move it again without asking.
        let (base, seen) = mock(fixture(), Some(areas())).await;
        let d = dirs("ha-cover-approve");
        let asked: Arc<Mutex<Vec<(String, String)>>> = Default::default();
        let run = |approve_ok: bool| {
            let base = base.clone();
            let d = d.clone();
            let asked = asked.clone();
            async move {
                set_with(
                    cfg(&base),
                    d,
                    move |domain: String, name: String| {
                        asked.lock().unwrap().push((domain, name));
                        std::future::ready(if approve_ok {
                            Ok(Box::new(()) as Guard)
                        } else {
                            Err(ha::approval_error(
                                &crate::commands::approval_gate::denied_message(),
                            ))
                        })
                    },
                    "cover.blind".into(),
                    "open_cover".into(),
                    None,
                    None,
                )
                .await
            }
        };
        assert!(run(false).await.unwrap_err().starts_with("declined:"));
        assert!(calls(&seen).is_empty());
        let r = run(true).await.unwrap();
        assert!(!r.undoable && r.id.is_none());
        assert_eq!(calls(&seen).len(), 1);
        let a = asked.lock().unwrap();
        assert_eq!(a.len(), 2);
        assert!(a.iter().all(|(dom, _)| dom == "cover"));
    }

    #[tokio::test]
    async fn a_scene_needs_a_native_approval_every_time() {
        let (base, seen) = mock(fixture(), Some(areas())).await;
        let d = dirs("ha-scene-approve");
        let asked: Arc<Mutex<Vec<String>>> = Default::default();
        let run = |approve_ok: bool, asked: Arc<Mutex<Vec<String>>>| {
            let base = base.clone();
            let d = d.clone();
            async move {
                set_with(
                    cfg(&base),
                    d,
                    move |_domain: String, name: String| {
                        asked.lock().unwrap().push(name);
                        std::future::ready(if approve_ok {
                            Ok(Box::new(()) as Guard)
                        } else {
                            Err(ha::approval_error(
                                &crate::commands::approval_gate::denied_message(),
                            ))
                        })
                    },
                    "scene.evening".into(),
                    "turn_on".into(),
                    None,
                    None,
                )
                .await
            }
        };
        let e = run(false, asked.clone()).await.unwrap_err();
        assert!(e.starts_with("declined:"));
        assert_eq!(asked.lock().unwrap().as_slice(), ["Evening"]);
        assert!(calls(&seen).is_empty());
        assert!(adds::history_in("ha", &d).unwrap().is_empty());
        assert!(run(true, asked.clone()).await.is_ok());
        assert!(run(true, asked.clone()).await.is_ok());
        assert_eq!(asked.lock().unwrap().len(), 3);
        assert_eq!(calls(&seen).len(), 2);
        // Lights never ask.
        let asked2: Arc<Mutex<Vec<String>>> = Default::default();
        set_with(
            cfg(&base),
            d.clone(),
            ok_approver(asked2.clone()),
            "light.kitchen".into(),
            "turn_off".into(),
            None,
            None,
        )
        .await
        .unwrap();
        assert!(asked2.lock().unwrap().is_empty());
        // A scene with a gate word is refused before any dialog.
        let asked3: Arc<Mutex<Vec<String>>> = Default::default();
        let e = set_with(
            cfg(&base),
            d.clone(),
            ok_approver(asked3.clone()),
            "scene.front_door_open".into(),
            "turn_on".into(),
            None,
            None,
        )
        .await
        .unwrap_err();
        assert!(e.starts_with("not_allowed:") && asked3.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_slow_ha_does_not_block_other_history_reads() {
        let (base, _) = mock_slow(fixture(), Some(areas()), 1500).await;
        let d = dirs("ha-slow");
        let (b2, d2) = (base.clone(), d.clone());
        let task =
            tokio::spawn(async move { set(&b2, &d2, "light.kitchen", "turn_off", None).await });
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        let d3 = d.clone();
        let t = std::time::Instant::now();
        tokio::task::spawn_blocking(move || adds::history_in("ha", &d3))
            .await
            .unwrap()
            .unwrap();
        assert!(
            t.elapsed() < std::time::Duration::from_millis(500),
            "History was blocked"
        );
        assert!(!task.is_finished());
        assert!(task.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn ha_slower_than_the_cap_times_out() {
        let (base, _) = mock_slow(fixture(), Some(areas()), 3500).await;
        let d = dirs("ha-timeout");
        let e = set(&base, &d, "light.kitchen", "turn_off", None)
            .await
            .unwrap_err();
        assert!(e.starts_with("timed_out:") && !e.contains(TOKEN));
        assert!(adds::history_in("ha", &d).unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_write_path_skips_the_area_template() {
        let (base, seen) = mock(fixture(), Some(areas())).await;
        let d = dirs("ha-norooms");
        set(&base, &d, "switch.heater", "turn_on", None)
            .await
            .unwrap();
        assert!(!seen
            .lock()
            .unwrap()
            .iter()
            .any(|(p, _)| p == "/api/template"));
    }

    #[tokio::test]
    async fn history_failure_after_a_good_call_is_reported_distinctly() {
        let (base, seen) = mock(fixture(), Some(areas())).await;
        let d = dirs("ha-notrec");
        std::fs::create_dir_all(d.data.join("adds").join("history.json")).unwrap();
        let e = set(&base, &d, "switch.heater", "turn_on", None)
            .await
            .unwrap_err();
        assert!(e.contains("applied but not recorded"), "{e}");
        assert_eq!(calls(&seen).len(), 1);
    }

    #[tokio::test]
    async fn undo_rechecks_gate_and_system() {
        let _g = SERIAL.lock().await;
        let (base, seen) = mock(fixture(), Some(areas())).await;
        *TEST_CFG.lock().unwrap() = Some(cfg(&base));
        for id in [
            "switch.garage_opener",
            "sensor.backup_manager",
            "light.nope",
        ] {
            let id = id.to_string();
            let r = tokio::task::spawn_blocking(move || {
                restore_blocking(&id, Some(&("turn_off".to_string(), None)))
            })
            .await
            .unwrap();
            assert!(r.is_err());
        }
        assert!(calls(&seen).is_empty());
        *TEST_CFG.lock().unwrap() = None;
    }

    #[test]
    fn generic_add_path_does_not_take_ha() {
        let d = dirs("ha-generic");
        let day = chrono::NaiveDate::from_ymd_opt(2026, 11, 18).unwrap();
        assert!(adds::preview_in("ha", "x", &d, day).is_err());
    }

    #[test]
    fn debug_of_results_has_no_secret_fields() {
        let src = include_str!("ha_devices.rs");
        let body = src.split("#[cfg(test)]").next().unwrap();
        assert!(!body.contains("println!") && !body.contains("eprintln!"));
    }

    // ─── level sliders ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn list_exposes_level_service_and_level() {
        let (base, _) = mock(fixture(), None).await;
        let r = devices_with(&cfg(&base), false).await.unwrap();
        let get = |id: &str| {
            r.devices
                .iter()
                .find(|d| d.entity_id == id)
                .unwrap()
                .clone()
        };
        let lv = |id: &str| {
            let d = get(id);
            (d.level_service, d.level, d.level_max)
        };
        assert_eq!(
            lv("light.kitchen"),
            (Some("turn_on".into()), Some(50.0), Some(100.0))
        );
        assert_eq!(lv("light.plain"), (None, None, None));
        assert_eq!(
            lv("fan.bed"),
            (Some("set_percentage".into()), Some(40.0), Some(100.0))
        );
        assert_eq!(lv("fan.basic"), (None, None, None));
        assert_eq!(
            lv("cover.blind"),
            (Some("set_cover_position".into()), Some(60.0), Some(100.0))
        );
        assert_eq!(lv("cover.nopos"), (None, None, None));
        assert_eq!(
            lv("media_player.tv"),
            (Some("volume_set".into()), Some(0.3), Some(1.0))
        );
        assert_eq!(lv("media_player.mute"), (None, None, None));
        assert!(get("cover.blind")
            .services
            .contains(&"set_cover_position".to_string()));
        assert!(!get("cover.nopos")
            .services
            .contains(&"set_cover_position".to_string()));
        assert!(!get("fan.basic")
            .services
            .contains(&"set_percentage".to_string()));
        assert!(!get("media_player.mute")
            .services
            .contains(&"volume_set".to_string()));
        // Camel case on the wire.
        let j = serde_json::to_value(get("fan.bed")).unwrap();
        assert_eq!(j["levelService"], json!("set_percentage"));
    }

    #[test]
    fn level_plan_accepts_in_range_whole_numbers_only() {
        let p = |d: &str, s: &str, v: Value| plan(d, s, Some(&v), None);
        // accepted
        assert!(p("light", "turn_on", json!(1)).is_ok());
        assert!(p("light", "turn_on", json!(100)).is_ok());
        assert!(p("fan", "set_percentage", json!(0)).is_ok());
        assert!(p("fan", "set_percentage", json!(100)).is_ok());
        assert!(p("cover", "set_cover_position", json!(0)).is_ok());
        assert!(p("cover", "set_cover_position", json!(100)).is_ok());
        assert!(p("media_player", "volume_set", json!(0.0)).is_ok());
        assert!(p("media_player", "volume_set", json!(1.0)).is_ok());
        let c = p("cover", "set_cover_position", json!(40)).ok().unwrap();
        assert_eq!(
            (c.ha_service, c.extra.get("position")),
            ("set_cover_position", Some(&json!(40.0)))
        );
        // out of range
        for (d, s, v) in [
            ("light", "turn_on", json!(0)),
            ("light", "turn_on", json!(101)),
            ("fan", "set_percentage", json!(-1)),
            ("fan", "set_percentage", json!(101)),
            ("cover", "set_cover_position", json!(101)),
            ("cover", "set_cover_position", json!(-5)),
            ("media_player", "volume_set", json!(1.01)),
            ("media_player", "volume_set", json!(-0.01)),
        ] {
            assert_eq!(p(d, s, v).err().unwrap(), ha::ERR_BAD_DATA, "{d} {s}");
        }
        // wrong type or fraction
        for (d, s, v) in [
            ("light", "turn_on", json!("50")),
            ("light", "turn_on", json!(50.5)),
            ("light", "turn_on", json!(true)),
            ("light", "turn_on", json!([50])),
            ("light", "turn_on", json!({"brightness_pct": 50})),
            ("fan", "set_percentage", json!("50")),
            ("cover", "set_cover_position", json!(12.5)),
            ("cover", "set_cover_position", json!(null)),
            ("media_player", "volume_set", json!("0.5")),
        ] {
            assert_eq!(p(d, s, v).err().unwrap(), ha::ERR_BAD_DATA, "{d} {s}");
        }
        assert!(plan("cover", "set_cover_position", None, None).is_err());
        // unknown key: a service that is not on the list
        for (d, s) in [
            ("light", "set_level"),
            ("light", "brightness_pct"),
            ("cover", "set_cover_tilt_position"),
            ("fan", "set_speed"),
            ("media_player", "volume_mute"),
            ("lock", "set_cover_position"),
        ] {
            assert_eq!(
                plan(d, s, Some(&json!(50)), None).err().unwrap(),
                ha::ERR_NOT_ALLOWED,
                "{d} {s}"
            );
        }
    }

    #[tokio::test]
    async fn level_writes_reach_ha_with_the_exact_body() {
        let (base, seen) = mock(fixture(), None).await;
        let d = dirs("ha-level");
        set(&base, &d, "light.kitchen", "turn_on", Some(json!(75)))
            .await
            .unwrap();
        set(&base, &d, "fan.bed", "set_percentage", Some(json!(60)))
            .await
            .unwrap();
        set(&base, &d, "media_player.tv", "volume_set", Some(json!(0.5)))
            .await
            .unwrap();
        let c: Vec<(String, Value)> = calls(&seen)
            .into_iter()
            .map(|(p, b)| (p, serde_json::from_str(&b).unwrap()))
            .collect();
        assert_eq!(
            c[0],
            (
                "/api/services/light/turn_on".into(),
                json!({"entity_id": "light.kitchen", "brightness_pct": 75.0})
            )
        );
        assert_eq!(
            c[1],
            (
                "/api/services/fan/set_percentage".into(),
                json!({"entity_id": "fan.bed", "percentage": 60.0})
            )
        );
        assert_eq!(
            c[2],
            (
                "/api/services/media_player/volume_set".into(),
                json!({"entity_id": "media_player.tv", "volume_level": 0.5})
            )
        );
    }

    #[tokio::test]
    async fn level_writes_are_refused_for_unsupported_devices() {
        let (base, seen) = mock(fixture(), None).await;
        let d = dirs("ha-level-unsupported");
        for (id, svc, v) in [
            ("light.plain", "turn_on", json!(50)),
            ("fan.basic", "set_percentage", json!(50)),
            ("cover.nopos", "set_cover_position", json!(50)),
            ("media_player.mute", "volume_set", json!(0.5)),
            // a service from another domain's list
            ("light.kitchen", "set_percentage", json!(50)),
        ] {
            let e = set(&base, &d, id, svc, Some(v)).await.unwrap_err();
            assert!(e.starts_with("not_allowed:"), "{id}: {e}");
        }
        // A plain on / off still works on an on/off light.
        set(&base, &d, "light.plain", "turn_off", None)
            .await
            .unwrap();
        assert_eq!(calls(&seen).len(), 1);
        // Out of range never reaches HA either.
        assert!(set(&base, &d, "light.kitchen", "turn_on", Some(json!(0)))
            .await
            .is_err());
        assert_eq!(calls(&seen).len(), 1);
    }

    #[tokio::test]
    async fn a_cover_position_asks_for_approval_and_has_no_undo() {
        let (base, seen) = mock(fixture(), None).await;
        let d = dirs("ha-cover-pos");
        let asked: Arc<Mutex<Vec<String>>> = Default::default();
        let r = set_with(
            cfg(&base),
            d.clone(),
            ok_approver(asked.clone()),
            "cover.blind".into(),
            "set_cover_position".into(),
            Some(json!(25)),
            None,
        )
        .await
        .unwrap();
        assert!(!r.undoable && r.id.is_none());
        assert_eq!(asked.lock().unwrap().len(), 1);
        let c = calls(&seen);
        assert_eq!(c[0].0, "/api/services/cover/set_cover_position");
        assert_eq!(
            serde_json::from_str::<Value>(&c[0].1).unwrap(),
            json!({"entity_id": "cover.blind", "position": 25.0})
        );
        // Declined: nothing sent.
        let denied = set_with(
            cfg(&base),
            d,
            |_d: String, _n: String| {
                std::future::ready(Err::<Guard, String>(ha::approval_error(
                    &crate::commands::approval_gate::denied_message(),
                )))
            },
            "cover.blind".into(),
            "set_cover_position".into(),
            Some(json!(25)),
            None,
        )
        .await;
        assert!(denied.unwrap_err().starts_with("declined:"));
        assert_eq!(calls(&seen).len(), 1);
    }

    #[tokio::test]
    async fn undo_restores_the_previous_level() {
        let _g = SERIAL.lock().await;
        let (base, seen) = mock(fixture(), None).await;
        *TEST_CFG.lock().unwrap() = Some(cfg(&base));
        let d = dirs("ha-level-undo");
        for (id, svc, v, back_path, key, back) in [
            (
                "light.kitchen",
                "turn_on",
                json!(90),
                "/api/services/light/turn_on",
                "brightness_pct",
                json!(50.0),
            ),
            (
                "fan.bed",
                "set_percentage",
                json!(90),
                "/api/services/fan/set_percentage",
                "percentage",
                json!(40.0),
            ),
            (
                "media_player.tv",
                "volume_set",
                json!(0.9),
                "/api/services/media_player/volume_set",
                "volume_level",
                json!(0.3),
            ),
        ] {
            let r = set(&base, &d, id, svc, Some(v)).await.unwrap();
            assert!(r.undoable, "{id}");
            let rid = r.id.unwrap();
            let d2 = d.clone();
            let ok = tokio::task::spawn_blocking(move || adds::undo_in("ha", &rid, &d2, now_jst()))
                .await
                .unwrap()
                .unwrap();
            assert!(ok);
            let last = calls(&seen).pop().unwrap();
            assert_eq!(last.0, back_path, "{id}");
            let body: Value = serde_json::from_str(&last.1).unwrap();
            assert_eq!(body[key], back, "{id}");
        }
        *TEST_CFG.lock().unwrap() = None;
    }

    #[test]
    fn restore_of_a_level_is_clamped_into_the_allowlist() {
        let mk = |state: &str, b: Option<f64>, p: Option<f64>, lvl: Option<&str>| HaDevice {
            entity_id: "x.y".into(),
            name: "N".into(),
            device_type: "light".into(),
            room: None,
            state: Some(state.into()),
            unit: None,
            system: false,
            controllable: true,
            services: vec![],
            brightness_pct: b,
            percentage: p,
            temperature: None,
            current_temperature: None,
            min_temperature: None,
            max_temperature: None,
            hvac_modes: vec![],
            volume: None,
            level_service: lvl.map(str::to_string),
            level: None,
            level_max: None,
        };
        // Brightness 0 % (HA brightness 1) restores as 1, and a fractional fan speed is rounded.
        let (s, v) = prior_call(
            "light",
            "turn_on",
            &mk("on", Some(0.0), None, Some("turn_on")),
        )
        .unwrap();
        assert_eq!(v, Some(json!(1.0)));
        assert!(plan("light", &s, v.as_ref(), None).is_ok());
        let (s, v) = prior_call(
            "fan",
            "set_percentage",
            &mk("on", None, Some(33.3), Some("set_percentage")),
        )
        .unwrap();
        assert_eq!(v, Some(json!(33.0)));
        assert!(plan("fan", &s, v.as_ref(), None).is_ok());
        // Off before: Undo turns it off.
        assert_eq!(
            prior_call("light", "turn_on", &mk("off", None, None, None))
                .unwrap()
                .0,
            "turn_off"
        );
    }

    // Security review LOW-1: a device that reports a level but doesn't accept level calls
    // (light with only "onoff", fan without SET_SPEED) gets a plain turn_on Undo, which its
    // re-check allows, instead of a level restore that the re-check would refuse.
    #[test]
    fn undo_of_a_device_without_level_support_is_a_plain_turn_on() {
        let mk = |kind: &str, b: Option<f64>, p: Option<f64>| HaDevice {
            entity_id: format!("{kind}.plain"),
            name: "N".into(),
            device_type: kind.into(),
            room: None,
            state: Some("on".into()),
            unit: None,
            system: false,
            controllable: true,
            services: vec![],
            brightness_pct: b,
            percentage: p,
            temperature: None,
            current_temperature: None,
            min_temperature: None,
            max_temperature: None,
            hvac_modes: vec![],
            volume: None,
            level_service: None,
            level: None,
            level_max: None,
        };
        let light = prior_call("light", "turn_off", &mk("light", Some(100.0), None)).unwrap();
        assert_eq!(light, ("turn_on".to_string(), None));
        let fan = prior_call("fan", "turn_off", &mk("fan", None, Some(40.0))).unwrap();
        assert_eq!(fan, ("turn_on".to_string(), None));
        assert!(plan("light", &light.0, light.1.as_ref(), None).is_ok());
        assert!(plan("fan", &fan.0, fan.1.as_ref(), None).is_ok());
    }
}
