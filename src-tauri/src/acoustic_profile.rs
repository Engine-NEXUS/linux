//! Adaptive Microphone Acoustic Profile
//!
//! Stores hardware-specific microphone calibration data (noise floor, fan rumble
//! frequency, adaptive highpass cutoff, pre-gain, and silence threshold).
//! Loaded by `wakeword_oww.rs` on startup to adapt DSP to the host laptop.

use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcousticProfile {
    #[serde(default = "default_device_name")]
    pub device_name: String,
    #[serde(default = "default_native_sample_rate")]
    pub native_sample_rate: u32,
    #[serde(default = "default_noise_floor")]
    pub measured_noise_floor_rms: f32,
    #[serde(default = "default_rumble_hz")]
    pub detected_fan_rumble_hz: f32,
    #[serde(default = "default_highpass_cutoff")]
    pub highpass_cutoff_hz: f32,
    #[serde(default = "default_pre_gain")]
    pub pre_gain: f32,
    #[serde(default = "default_silence_threshold")]
    pub silence_rms_threshold: f32,
    #[serde(default = "default_impulsive_ratio")]
    pub impulsive_ratio: f32,
    #[serde(default = "default_kws_threshold")]
    pub kws_threshold: f32,
    #[serde(default)]
    pub calibrated_at: String,
}

fn default_device_name() -> String {
    "Default Microphone".to_string()
}
fn default_native_sample_rate() -> u32 {
    48000
}
fn default_noise_floor() -> f32 {
    0.001
}
fn default_rumble_hz() -> f32 {
    80.0
}
fn default_highpass_cutoff() -> f32 {
    80.0
}
fn default_pre_gain() -> f32 {
    1.5
}
fn default_silence_threshold() -> f32 {
    0.003
}
fn default_impulsive_ratio() -> f32 {
    8.0
}
fn default_kws_threshold() -> f32 {
    0.50
}

impl Default for AcousticProfile {
    fn default() -> Self {
        Self {
            device_name: default_device_name(),
            native_sample_rate: default_native_sample_rate(),
            measured_noise_floor_rms: default_noise_floor(),
            detected_fan_rumble_hz: default_rumble_hz(),
            highpass_cutoff_hz: default_highpass_cutoff(),
            pre_gain: default_pre_gain(),
            silence_rms_threshold: default_silence_threshold(),
            impulsive_ratio: default_impulsive_ratio(),
            kws_threshold: default_kws_threshold(),
            calibrated_at: String::new(),
        }
    }
}

impl AcousticProfile {
    /// Attempt to load the acoustic profile from %APPDATA% or resources directory.
    /// Falls back to default if no profile is found or if parsing fails.
    pub fn load_or_default(resource_dir: Option<&Path>) -> Self {
        if let Some(profile) = Self::try_load_appdata() {
            tracing::info!(
                "acoustic_profile: loaded from AppData for '{}' (HP: {:.1}Hz, Pre-Gain: {:.2}x, Gate: {:.5}, Threshold: {:.2})",
                profile.device_name,
                profile.highpass_cutoff_hz,
                profile.pre_gain,
                profile.silence_rms_threshold,
                profile.kws_threshold
            );
            return profile;
        }

        if let Some(res_dir) = resource_dir {
            if let Some(profile) = Self::try_load_from_path(&res_dir.join("acoustic_profile.json")) {
                tracing::info!(
                    "acoustic_profile: loaded from resource_dir for '{}' (HP: {:.1}Hz, Pre-Gain: {:.2}x)",
                    profile.device_name,
                    profile.highpass_cutoff_hz,
                    profile.pre_gain
                );
                return profile;
            }
            if let Some(profile) = Self::try_load_from_path(&res_dir.join("resources").join("oww").join("acoustic_profile.json")) {
                tracing::info!("acoustic_profile: loaded from resources/oww");
                return profile;
            }
        }

        // Dev mode fallback
        let dev_path = PathBuf::from("resources/oww/acoustic_profile.json");
        if let Some(profile) = Self::try_load_from_path(&dev_path) {
            tracing::info!("acoustic_profile: loaded from dev path");
            return profile;
        }

        tracing::info!("acoustic_profile: no profile found, using balanced defaults");
        Self::default()
    }

    fn try_load_appdata() -> Option<Self> {
        #[cfg(target_os = "windows")]
        {
            if let Ok(appdata) = std::env::var("APPDATA") {
                let path = PathBuf::from(appdata)
                    .join("com.nexus.assistant")
                    .join("acoustic_profile.json");
                return Self::try_load_from_path(&path);
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            if let Ok(home) = std::env::var("HOME") {
                let path = PathBuf::from(home)
                    .join(".config")
                    .join("com.nexus.assistant")
                    .join("acoustic_profile.json");
                return Self::try_load_from_path(&path);
            }
        }

        None
    }

    fn try_load_from_path(path: &Path) -> Option<Self> {
        if !path.exists() {
            return None;
        }
        let data = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_acoustic_profile_defaults() {
        let profile = AcousticProfile::default();
        assert_eq!(profile.highpass_cutoff_hz, 80.0);
        assert_eq!(profile.pre_gain, 1.5);
        assert_eq!(profile.silence_rms_threshold, 0.003);
        assert_eq!(profile.impulsive_ratio, 8.0);
        assert_eq!(profile.kws_threshold, 0.50);
    }

    #[test]
    fn test_acoustic_profile_json_roundtrip() {
        let json_str = r#"{
            "device_name": "Test Laptop MEMS Mic",
            "native_sample_rate": 44100,
            "measured_noise_floor_rms": 0.00004,
            "detected_fan_rumble_hz": 113.3,
            "highpass_cutoff_hz": 128.3,
            "pre_gain": 2.5,
            "silence_rms_threshold": 0.0032,
            "impulsive_ratio": 8.0,
            "kws_threshold": 0.50
        }"#;

        let profile: AcousticProfile = serde_json::from_str(json_str).expect("deserialize failed");
        assert_eq!(profile.device_name, "Test Laptop MEMS Mic");
        assert_eq!(profile.highpass_cutoff_hz, 128.3);
        assert_eq!(profile.pre_gain, 2.5);
        assert_eq!(profile.kws_threshold, 0.50);
    }
}
