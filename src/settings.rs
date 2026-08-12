use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub const DEFAULT_OPACITY: f64 = 0.94;
pub const MIN_OPACITY: f64 = 0.35;
pub const START_MINIMISED_ARG: &str = "--start-minimised";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommonSettings {
    pub opacity: f64,
    pub always_on_top: bool,
    pub start_minimised: bool,
}

impl Default for CommonSettings {
    fn default() -> Self {
        Self {
            opacity: DEFAULT_OPACITY,
            always_on_top: false,
            start_minimised: false,
        }
    }
}

/// Full settings document: common keys + arbitrary app fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedSettings {
    pub opacity: f64,
    pub always_on_top: bool,
    pub start_minimised: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_bounds: Option<Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl PersistedSettings {
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    pub fn common(&self) -> CommonSettings {
        CommonSettings {
            opacity: clamp_opacity(self.opacity),
            always_on_top: self.always_on_top,
            start_minimised: self.start_minimised,
        }
    }

    pub fn merge_partial(&mut self, partial: &Value) {
        let Some(obj) = partial.as_object() else {
            return;
        };
        for (k, v) in obj {
            match k.as_str() {
                "opacity" => {
                    if let Some(n) = v.as_f64() {
                        self.opacity = clamp_opacity(n);
                    }
                }
                "alwaysOnTop" => {
                    if let Some(b) = v.as_bool() {
                        self.always_on_top = b;
                    }
                }
                "startMinimised" => {
                    if let Some(b) = v.as_bool() {
                        self.start_minimised = b;
                    }
                }
                "windowBounds" => {
                    self.window_bounds = Some(v.clone());
                }
                _ => {
                    self.extra.insert(k.clone(), v.clone());
                }
            }
        }
    }
}

pub fn clamp_opacity(value: f64) -> f64 {
    if !value.is_finite() {
        return DEFAULT_OPACITY;
    }
    value.clamp(MIN_OPACITY, 1.0)
}

pub fn load_or_create(
    path: &Path,
    defaults: &HashMap<String, Value>,
) -> Result<PersistedSettings, std::io::Error> {
    if path.exists() {
        let raw = fs::read_to_string(path)?;
        if let Ok(mut settings) = serde_json::from_str::<PersistedSettings>(&raw) {
            settings.opacity = clamp_opacity(settings.opacity);
            return Ok(settings);
        }
    }

    let mut settings = PersistedSettings {
        opacity: DEFAULT_OPACITY,
        always_on_top: false,
        start_minimised: false,
        window_bounds: None,
        extra: Map::new(),
    };

    for (k, v) in defaults {
        match k.as_str() {
            "opacity" => {
                if let Some(n) = v.as_f64() {
                    settings.opacity = clamp_opacity(n);
                }
            }
            "alwaysOnTop" => {
                settings.always_on_top = v.as_bool().unwrap_or(false);
            }
            "startMinimised" => {
                settings.start_minimised = v.as_bool().unwrap_or(false);
            }
            "windowBounds" => {
                settings.window_bounds = Some(v.clone());
            }
            _ => {
                settings.extra.insert(k.clone(), v.clone());
            }
        }
    }

    save(path, &settings)?;
    Ok(settings)
}

pub fn save_settings(path: &Path, settings: &PersistedSettings) -> Result<(), std::io::Error> {
    save(path, settings)
}

pub fn save(path: &Path, settings: &PersistedSettings) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let pretty = serde_json::to_string_pretty(settings)
        .unwrap_or_else(|_| json!({}).to_string());
    fs::write(path, pretty)
}
