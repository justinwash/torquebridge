use crate::backends::directinput::DirectInput;
use crate::config::{ControllerConfig, FfbParamsConfig, load_controllers};
use crate::core::domain::{DeviceControlCommand, WheelCommand};
use crate::diagnostics::{
    BridgeDiagnosticEvent, DiagnosticCategory, DiagnosticField, DiagnosticLevel, DiagnosticsLog,
    format_event,
};
use crate::inputs::{WinmmDeviceInfo, WinmmJoystick};
use crate::profile::{FfbProfile, load_profile, save_profile};
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use slint::{
    CloseRequestResponse, ComponentHandle, ModelRc, SharedString, Timer, TimerMode, VecModel,
};
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::time::Duration;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

slint::include_modules!();

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const DIAGNOSTIC_EVENT_LIMIT: usize = 48;
const OBSERVABILITY_EVENT_LIMIT: usize = 128;

#[derive(Debug, Clone, PartialEq)]
struct ProfileEditorState {
    profile_name: String,
    notes: String,
    poll_ms: f32,
    steering_device: f32,
    const_magnitude: f32,
    const_maximum_force: f32,
    const_minimum_force: f32,
    const_filter_threshold: f32,
    const_minimum_coefficient: f32,
    sine_magnitude: f32,
    sine_frequency: f32,
    sine_maximum_force: f32,
    sine_phase: f32,
    engine_vibration_strength: f32,
    gear_shift_strength: f32,
    spring_coefficient: f32,
    spring_saturation: f32,
    damper_coefficient: f32,
    damper_saturation: f32,
}

impl From<&FfbProfile> for ProfileEditorState {
    fn from(profile: &FfbProfile) -> Self {
        Self {
            profile_name: profile
                .name
                .clone()
                .unwrap_or_else(|| "Untitled Profile".to_string()),
            notes: profile.notes.clone().unwrap_or_default(),
            poll_ms: profile.poll_ms.unwrap_or(5) as f32,
            steering_device: profile
                .steering_device
                .map(|value| value as f32)
                .unwrap_or(-1.0),
            const_magnitude: profile.ffb_parameters.r#const.magnitude,
            const_maximum_force: profile.ffb_parameters.r#const.maximum_force,
            const_minimum_force: profile.ffb_parameters.r#const.minimum_force,
            const_filter_threshold: profile.ffb_parameters.r#const.filter_threshold,
            const_minimum_coefficient: profile.ffb_parameters.r#const.minimum_coefficient,
            sine_magnitude: profile.ffb_parameters.sine.magnitude,
            sine_frequency: profile.ffb_parameters.sine.frequency,
            sine_maximum_force: profile.ffb_parameters.sine.maximum_force,
            sine_phase: profile.ffb_parameters.sine.phase,
            engine_vibration_strength: profile.ffb_parameters.sine.engine_vibrations.strength,
            gear_shift_strength: profile.ffb_parameters.sine.gear_shift_vibrations.strength,
            spring_coefficient: profile.ffb_parameters.spring.coefficient,
            spring_saturation: profile.ffb_parameters.spring.saturation,
            damper_coefficient: profile.ffb_parameters.damper.coefficient,
            damper_saturation: profile.ffb_parameters.damper.saturation,
        }
    }
}

impl ProfileEditorState {
    fn apply_to_window(&self, window: &ProfileEditorWindow, profile_path: &Path) {
        window.set_profile_name(SharedString::from(self.profile_name.clone()));
        window.set_notes(SharedString::from(self.notes.clone()));
        window.set_profile_path(SharedString::from(profile_path.display().to_string()));
        window.set_poll_ms(self.poll_ms);
        window.set_steering_device(self.steering_device);
        window.set_const_magnitude(self.const_magnitude);
        window.set_const_maximum_force(self.const_maximum_force);
        window.set_const_minimum_force(self.const_minimum_force);
        window.set_const_filter_threshold(self.const_filter_threshold);
        window.set_const_minimum_coefficient(self.const_minimum_coefficient);
        window.set_sine_magnitude(self.sine_magnitude);
        window.set_sine_frequency(self.sine_frequency);
        window.set_sine_maximum_force(self.sine_maximum_force);
        window.set_sine_phase(self.sine_phase);
        window.set_engine_vibration_strength(self.engine_vibration_strength);
        window.set_gear_shift_strength(self.gear_shift_strength);
        window.set_spring_coefficient(self.spring_coefficient);
        window.set_spring_saturation(self.spring_saturation);
        window.set_damper_coefficient(self.damper_coefficient);
        window.set_damper_saturation(self.damper_saturation);
    }

    fn from_window(window: &ProfileEditorWindow) -> Self {
        Self {
            profile_name: window.get_profile_name().to_string(),
            notes: window.get_notes().to_string(),
            poll_ms: window.get_poll_ms(),
            steering_device: window.get_steering_device(),
            const_magnitude: window.get_const_magnitude(),
            const_maximum_force: window.get_const_maximum_force(),
            const_minimum_force: window.get_const_minimum_force(),
            const_filter_threshold: window.get_const_filter_threshold(),
            const_minimum_coefficient: window.get_const_minimum_coefficient(),
            sine_magnitude: window.get_sine_magnitude(),
            sine_frequency: window.get_sine_frequency(),
            sine_maximum_force: window.get_sine_maximum_force(),
            sine_phase: window.get_sine_phase(),
            engine_vibration_strength: window.get_engine_vibration_strength(),
            gear_shift_strength: window.get_gear_shift_strength(),
            spring_coefficient: window.get_spring_coefficient(),
            spring_saturation: window.get_spring_saturation(),
            damper_coefficient: window.get_damper_coefficient(),
            damper_saturation: window.get_damper_saturation(),
        }
    }

    fn into_profile(self, base: &FfbProfile) -> FfbProfile {
        let mut profile = base.clone();
        profile.name = if self.profile_name.trim().is_empty() {
            None
        } else {
            Some(self.profile_name.trim().to_string())
        };
        profile.notes = if self.notes.trim().is_empty() {
            None
        } else {
            Some(self.notes.trim().to_string())
        };
        profile.poll_ms = Some(self.poll_ms.round().clamp(1.0, 20.0) as u64);
        profile.steering_device = if self.steering_device < 0.0 {
            None
        } else {
            Some(self.steering_device.round().clamp(0.0, 32.0) as u32)
        };
        profile.ffb_parameters.r#const.magnitude = self.const_magnitude.clamp(0.0, 2.0);
        profile.ffb_parameters.r#const.maximum_force = self.const_maximum_force.clamp(0.0, 1.0);
        profile.ffb_parameters.r#const.minimum_force = self.const_minimum_force.clamp(0.0, 0.25);
        profile.ffb_parameters.r#const.filter_threshold =
            self.const_filter_threshold.clamp(0.0, 1.0);
        profile.ffb_parameters.r#const.minimum_coefficient =
            self.const_minimum_coefficient.clamp(0.0, 1.0);
        profile.ffb_parameters.sine.magnitude = self.sine_magnitude.clamp(0.0, 1.5);
        profile.ffb_parameters.sine.frequency = self.sine_frequency.clamp(0.25, 4.0);
        profile.ffb_parameters.sine.maximum_force = self.sine_maximum_force.clamp(0.0, 1.0);
        profile.ffb_parameters.sine.phase = self.sine_phase.clamp(0.0, 1.0);
        profile.ffb_parameters.sine.engine_vibrations.strength =
            self.engine_vibration_strength.clamp(0.0, 0.25);
        profile.ffb_parameters.sine.gear_shift_vibrations.strength =
            self.gear_shift_strength.clamp(0.0, 0.25);
        profile.ffb_parameters.spring.coefficient = self.spring_coefficient.clamp(0.0, 0.25);
        profile.ffb_parameters.spring.saturation = self.spring_saturation.clamp(0.0, 1.0);
        profile.ffb_parameters.damper.coefficient = self.damper_coefficient.clamp(0.0, 0.25);
        profile.ffb_parameters.damper.saturation = self.damper_saturation.clamp(0.0, 1.0);
        profile
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
struct EditorSettings {
    hot_reload_enabled: bool,
    close_to_taskbar_enabled: bool,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            hot_reload_enabled: true,
            close_to_taskbar_enabled: false,
        }
    }
}

impl EditorSettings {
    fn apply_to_window(&self, window: &ProfileEditorWindow) {
        window.set_hot_reload_enabled(self.hot_reload_enabled);
        window.set_close_to_taskbar_enabled(self.close_to_taskbar_enabled);
    }

    fn from_window(window: &ProfileEditorWindow) -> Self {
        Self {
            hot_reload_enabled: window.get_hot_reload_enabled(),
            close_to_taskbar_enabled: window.get_close_to_taskbar_enabled(),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct DeviceCatalog {
    devices: Vec<WinmmDeviceInfo>,
    load_error: Option<String>,
}

impl DeviceCatalog {
    fn load() -> Self {
        match WinmmJoystick::list_devices() {
            Ok(devices) => Self {
                devices,
                load_error: None,
            },
            Err(error) => Self {
                devices: Vec::new(),
                load_error: Some(error.to_string()),
            },
        }
    }

    fn model(&self) -> ModelRc<SharedString> {
        let mut options = vec![SharedString::from("No steering device")];
        options.extend(
            self.devices
                .iter()
                .map(|device| SharedString::from(Self::device_label(device))),
        );
        ModelRc::new(VecModel::from(options))
    }

    fn empty_message(&self) -> SharedString {
        if self.devices.is_empty() {
            SharedString::from(
                self.load_error
                    .as_ref()
                    .map(|error| format!("Device names unavailable: {error}"))
                    .unwrap_or_else(|| "No WinMM devices found.".to_string()),
            )
        } else {
            SharedString::from("")
        }
    }

    fn selected_index(&self, device_id: Option<u32>) -> i32 {
        device_id
            .and_then(|id| self.devices.iter().position(|device| device.id == id))
            .map(|index| index as i32 + 1)
            .unwrap_or(0)
    }

    fn device_id_for_index(&self, index: i32) -> Option<u32> {
        let row = usize::try_from(index).ok()?;
        if row == 0 {
            return None;
        }

        self.devices.get(row - 1).map(|device| device.id)
    }

    fn label_for_device(&self, device_id: Option<u32>) -> String {
        match device_id {
            None => "No steering device".to_string(),
            Some(id) => self
                .devices
                .iter()
                .find(|device| device.id == id)
                .map(Self::device_label)
                .unwrap_or_else(|| format!("WinMM #{} (not found)", id)),
        }
    }

    fn device_label(device: &WinmmDeviceInfo) -> String {
        format!("{} [WinMM #{}]", device.name, device.id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ObservabilitySource {
    Live,
    Replay(PathBuf),
}

#[derive(Debug, Clone)]
struct ObservabilityController {
    live_log: DiagnosticsLog,
    source: ObservabilitySource,
}

impl ObservabilityController {
    fn new(live_log: DiagnosticsLog) -> Self {
        Self {
            live_log,
            source: ObservabilitySource::Live,
        }
    }

    fn current_log(&self) -> DiagnosticsLog {
        match &self.source {
            ObservabilitySource::Live => self.live_log.clone(),
            ObservabilitySource::Replay(path) => DiagnosticsLog::new(path),
        }
    }

    fn source_label(&self) -> String {
        match &self.source {
            ObservabilitySource::Live => "Source: live session log".to_string(),
            ObservabilitySource::Replay(path) => {
                format!("Source: replaying {}", replay_source_label(path))
            }
        }
    }

    fn export_live_session(&self) -> Result<PathBuf> {
        self.live_log.export_session()
    }

    fn replay_latest_session(&mut self) -> Result<PathBuf> {
        let replay_path = match self.live_log.latest_exported_session() {
            Ok(path) => path,
            Err(_) => self.live_log.export_session()?,
        };
        self.source = ObservabilitySource::Replay(replay_path.clone());
        Ok(replay_path)
    }

    fn return_to_live(&mut self) -> bool {
        let was_replaying = matches!(self.source, ObservabilitySource::Replay(_));
        self.source = ObservabilitySource::Live;
        was_replaying
    }
}

fn replay_source_label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_string())
        .unwrap_or_else(|| path.display().to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TelemetryPanelView {
    status: String,
    uptime: String,
    packet_rate: String,
    command_rate: String,
    steering: String,
    peak_force: String,
    clamp_status: String,
    saturation_status: String,
    packet_trend: String,
    command_trend: String,
    force_trend: String,
    runtime_detail: String,
    last_update: String,
    last_commands: String,
}

impl TelemetryPanelView {
    fn waiting(source_label: &str) -> Self {
        Self {
            status: format!("{source_label}. Waiting for bridge telemetry."),
            uptime: "Idle".to_string(),
            packet_rate: "0.0/s".to_string(),
            command_rate: "0.0/s".to_string(),
            steering: "Unavailable".to_string(),
            peak_force: "0%".to_string(),
            clamp_status: "No active force output.".to_string(),
            saturation_status: "No condition saturation activity.".to_string(),
            packet_trend: "No packet-rate trend yet.".to_string(),
            command_trend: "No command-rate trend yet.".to_string(),
            force_trend: "No force trend yet.".to_string(),
            runtime_detail:
                "Launch the bridge to capture input, output, poll interval, and hot-reload state."
                    .to_string(),
            last_update: "No translated update yet.".to_string(),
            last_commands: "No applied command summary yet.".to_string(),
        }
    }

    fn failed(source_label: &str, error: &str) -> Self {
        Self {
            status: format!("{source_label}. Telemetry unavailable."),
            uptime: "Read failed".to_string(),
            packet_rate: "Read failed".to_string(),
            command_rate: "Read failed".to_string(),
            steering: "Read failed".to_string(),
            peak_force: "Read failed".to_string(),
            clamp_status: format!("Failed to read telemetry log: {error}"),
            saturation_status: format!("Failed to read telemetry log: {error}"),
            packet_trend: format!("Failed to read telemetry log: {error}"),
            command_trend: format!("Failed to read telemetry log: {error}"),
            force_trend: format!("Failed to read telemetry log: {error}"),
            runtime_detail: format!("Failed to read telemetry log: {error}"),
            last_update: format!("Failed to read telemetry log: {error}"),
            last_commands: format!("Failed to read telemetry log: {error}"),
        }
    }

    fn from_events(events: &[BridgeDiagnosticEvent], source_label: &str) -> Self {
        let telemetry_events = events
            .iter()
            .filter(|event| event.category == DiagnosticCategory::Telemetry)
            .collect::<Vec<_>>();
        let Some(event) = telemetry_events.last().copied() else {
            return Self::waiting(source_label);
        };

        let trend_events = if telemetry_events.len() > 24 {
            &telemetry_events[telemetry_events.len() - 24..]
        } else {
            &telemetry_events
        };

        let uptime = telemetry_field(event, "uptime", "Waiting");
        let packet_rate = telemetry_field(event, "packet_rate", "0.0/s");
        let command_rate = telemetry_field(event, "command_rate", "0.0/s");
        let steering = telemetry_field(event, "steering", "Unavailable");
        let peak_force = telemetry_field(event, "peak_force", "0%");
        let clamp_status = telemetry_field(event, "clamp_status", "No active force output.");
        let saturation_status = telemetry_field(
            event,
            "saturation_status",
            "No condition saturation activity.",
        );
        let last_packet_age = telemetry_field(event, "last_packet_age", "No packet yet");
        let input = telemetry_field(event, "input", "No steering input");
        let output = telemetry_field(event, "output", "Unknown output");
        let poll_ms = telemetry_field(event, "poll_ms", "Unknown poll");
        let hot_reload = telemetry_field(event, "hot_reload", "Unknown");
        let updates_total = telemetry_field(event, "updates_total", "0");
        let commands_total = telemetry_field(event, "commands_total", "0");

        Self {
            status: format!("{source_label}. Last packet {last_packet_age}."),
            uptime,
            packet_rate,
            command_rate,
            steering,
            peak_force,
            clamp_status,
            saturation_status,
            packet_trend: telemetry_rate_trend(trend_events, "packet_rate_value", "/s"),
            command_trend: telemetry_rate_trend(trend_events, "command_rate_value", "/s"),
            force_trend: telemetry_percent_trend(trend_events, "peak_force_ratio"),
            runtime_detail: format!(
                "Input: {input}\nOutput: {output}\nPoll interval: {poll_ms}\nHot reload: {hot_reload}\nTotals: {updates_total} updates / {commands_total} commands"
            ),
            last_update: telemetry_field(
                event,
                "last_update",
                "No translated update captured yet.",
            ),
            last_commands: telemetry_field(
                event,
                "last_commands",
                "No applied commands captured yet.",
            ),
        }
    }

    fn apply_to_window(&self, window: &ProfileEditorWindow) {
        window.set_telemetry_status(SharedString::from(self.status.clone()));
        window.set_telemetry_uptime(SharedString::from(self.uptime.clone()));
        window.set_telemetry_packet_rate(SharedString::from(self.packet_rate.clone()));
        window.set_telemetry_command_rate(SharedString::from(self.command_rate.clone()));
        window.set_telemetry_steering(SharedString::from(self.steering.clone()));
        window.set_telemetry_peak_force(SharedString::from(self.peak_force.clone()));
        window.set_telemetry_clamp_status(SharedString::from(self.clamp_status.clone()));
        window.set_telemetry_saturation_status(SharedString::from(self.saturation_status.clone()));
        window.set_telemetry_packet_trend(SharedString::from(self.packet_trend.clone()));
        window.set_telemetry_command_trend(SharedString::from(self.command_trend.clone()));
        window.set_telemetry_force_trend(SharedString::from(self.force_trend.clone()));
        window.set_telemetry_runtime_detail(SharedString::from(self.runtime_detail.clone()));
        window.set_telemetry_last_update(SharedString::from(self.last_update.clone()));
        window.set_telemetry_last_commands(SharedString::from(self.last_commands.clone()));
    }
}

fn telemetry_field(event: &BridgeDiagnosticEvent, key: &str, fallback: &str) -> String {
    event.field_value(key).unwrap_or(fallback).to_string()
}

fn telemetry_numeric_field(event: &BridgeDiagnosticEvent, key: &str) -> Option<f32> {
    event.field_value(key)?.parse::<f32>().ok()
}

fn telemetry_numeric_series(events: &[&BridgeDiagnosticEvent], key: &str) -> Vec<f32> {
    events
        .iter()
        .filter_map(|event| telemetry_numeric_field(event, key))
        .collect()
}

fn telemetry_rate_trend(events: &[&BridgeDiagnosticEvent], key: &str, unit: &str) -> String {
    let series = telemetry_numeric_series(events, key);
    if series.is_empty() {
        return "No trend yet.".to_string();
    }

    let peak = series.iter().copied().fold(0.0f32, f32::max);
    format!("{}  peak {:.1}{unit}", ascii_sparkline(&series), peak)
}

fn telemetry_percent_trend(events: &[&BridgeDiagnosticEvent], key: &str) -> String {
    let series = telemetry_numeric_series(events, key);
    if series.is_empty() {
        return "No trend yet.".to_string();
    }

    let peak = series.iter().copied().fold(0.0f32, f32::max) * 100.0;
    format!("{}  peak {:.0}%", ascii_sparkline(&series), peak)
}

fn ascii_sparkline(samples: &[f32]) -> String {
    const GLYPHS: &[u8] = b" .:-=+*#%@";

    if samples.is_empty() {
        return "-".to_string();
    }

    let max_value = samples.iter().copied().fold(0.0f32, f32::max).max(1.0);
    samples
        .iter()
        .map(|sample| {
            let normalized = (*sample / max_value).clamp(0.0, 1.0);
            let index = (normalized * (GLYPHS.len() - 1) as f32).round() as usize;
            GLYPHS[index] as char
        })
        .collect()
}

fn selected_steering_device(window: &ProfileEditorWindow) -> Option<u32> {
    let steering_device = window.get_steering_device().round() as i32;
    (steering_device >= 0).then_some(steering_device as u32)
}

fn hot_reload_summary(enabled: bool) -> &'static str {
    if enabled { "On save" } else { "Manual" }
}

fn editor_settings_path() -> Result<PathBuf> {
    if let Some(base_dir) = std::env::var_os("APPDATA").or_else(|| std::env::var_os("LOCALAPPDATA"))
    {
        return Ok(PathBuf::from(base_dir)
            .join("Torquebridge")
            .join("editor-settings.json"));
    }

    Ok(std::env::current_dir()
        .context("failed to resolve current directory for editor settings")?
        .join(".torquebridge-editor-settings.json"))
}

fn load_editor_settings(path: &Path) -> (EditorSettings, Option<String>) {
    match fs::read_to_string(path) {
        Ok(json) => {
            match serde_json::from_str::<EditorSettings>(json.trim_start_matches('\u{feff}')) {
                Ok(settings) => (settings, None),
                Err(_) => (
                    EditorSettings::default(),
                    Some("Editor settings reset to defaults.".to_string()),
                ),
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (EditorSettings::default(), None)
        }
        Err(_) => (
            EditorSettings::default(),
            Some("Editor settings unavailable; using defaults.".to_string()),
        ),
    }
}

fn save_editor_settings(path: &Path, settings: &EditorSettings) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("failed to create editor settings directory")?;
    }

    let json =
        serde_json::to_string_pretty(settings).context("failed to serialize editor settings")?;
    fs::write(path, format!("{json}\n")).context("failed to write editor settings")?;
    Ok(())
}

fn save_window_editor_settings(
    window: &ProfileEditorWindow,
    editor_settings: &RefCell<EditorSettings>,
    path: &Path,
) -> Result<EditorSettings> {
    let settings = EditorSettings::from_window(window);
    save_editor_settings(path, &settings)?;
    *editor_settings.borrow_mut() = settings.clone();
    Ok(settings)
}

fn initial_status_message(
    settings_notice: Option<String>,
    device_catalog: &DeviceCatalog,
) -> String {
    match (settings_notice, device_catalog.load_error.as_ref()) {
        (Some(settings_notice), Some(_)) => format!("{settings_notice} Device names unavailable."),
        (Some(settings_notice), None) => settings_notice,
        (None, Some(_)) => "Save writes the profile. Device names unavailable.".to_string(),
        (None, None) => "Save writes the profile.".to_string(),
    }
}

#[derive(Debug)]
struct BridgeController {
    config_path: String,
    profile_path: PathBuf,
    diagnostics_log: DiagnosticsLog,
    child: Option<Child>,
    detail: String,
}

impl BridgeController {
    fn new(config_path: String, profile_path: PathBuf, diagnostics_log: DiagnosticsLog) -> Self {
        Self {
            config_path,
            profile_path,
            diagnostics_log,
            child: None,
            detail: "Idle.".to_string(),
        }
    }

    fn is_running(&self) -> bool {
        self.child.is_some()
    }

    fn status_label(&self) -> &'static str {
        if self.is_running() {
            "RUNNING"
        } else {
            "STOPPED"
        }
    }

    fn detail(&self) -> &str {
        &self.detail
    }

    fn start(&mut self, hot_reload_enabled: bool) -> Result<()> {
        if self.child.is_some() {
            self.detail = "Already running.".to_string();
            record_ui_event(
                &self.diagnostics_log,
                DiagnosticLevel::Warning,
                DiagnosticCategory::Lifecycle,
                "Bridge launch requested while bridge is already running",
                Vec::new(),
            );
            return Ok(());
        }

        self.diagnostics_log.clear()?;
        let safety_detail =
            apply_bridge_safety_reset(&self.config_path, &self.diagnostics_log, "Prelaunch")?;

        let executable = std::env::current_exe().context("failed to locate current executable")?;
        let mut command = Command::new(executable);
        command
            .arg("ffb-bridge")
            .arg("--config")
            .arg(&self.config_path)
            .arg("--profile")
            .arg(&self.profile_path)
            .arg("--diagnostics-log")
            .arg(self.diagnostics_log.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        if !hot_reload_enabled {
            command.arg("--no-profile-watch");
        }

        #[cfg(windows)]
        {
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let child = command.spawn().context("failed to launch bridge process")?;
        let pid = child.id();
        self.child = Some(child);
        self.detail = if hot_reload_enabled {
            format!("PID {pid}. Hot reload on. {safety_detail}")
        } else {
            format!("PID {pid}. Hot reload off. {safety_detail}")
        };
        record_ui_event(
            &self.diagnostics_log,
            DiagnosticLevel::Info,
            DiagnosticCategory::Lifecycle,
            "Launched bridge process",
            vec![
                DiagnosticField::new("pid", pid.to_string()),
                DiagnosticField::new(
                    "hot_reload",
                    if hot_reload_enabled {
                        "enabled"
                    } else {
                        "disabled"
                    },
                ),
                DiagnosticField::new("profile", self.profile_path.display().to_string()),
            ],
        );
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        if let Some(mut child) = self.child.take() {
            let pid = child.id();
            if child
                .try_wait()
                .context("failed to check bridge process before stopping")?
                .is_none()
            {
                child.kill().context("failed to stop bridge process")?;
            }
            let _ = child.wait();

            let safety_detail =
                apply_bridge_safety_reset(&self.config_path, &self.diagnostics_log, "Shutdown")?;
            self.detail = format!("Stopped PID {pid}. {safety_detail}");
            record_ui_event(
                &self.diagnostics_log,
                DiagnosticLevel::Info,
                DiagnosticCategory::Lifecycle,
                "Stopped bridge process",
                vec![DiagnosticField::new("pid", pid.to_string())],
            );
        } else {
            self.detail = "Already stopped.".to_string();
        }
        Ok(())
    }

    fn poll(&mut self) -> Result<Option<String>> {
        let Some(child) = self.child.as_mut() else {
            return Ok(None);
        };

        if let Some(status) = child.try_wait().context("failed to poll bridge process")? {
            let pid = child.id();
            self.child = None;
            let safety_detail =
                apply_bridge_safety_reset(&self.config_path, &self.diagnostics_log, "Exit")?;
            self.detail = format!("Exited: {status}. {safety_detail}");
            record_ui_event(
                &self.diagnostics_log,
                DiagnosticLevel::Warning,
                DiagnosticCategory::Lifecycle,
                "Bridge process exited",
                vec![
                    DiagnosticField::new("pid", pid.to_string()),
                    DiagnosticField::new("status", status.to_string()),
                ],
            );
            return Ok(Some(self.detail.clone()));
        }

        Ok(None)
    }
}

pub fn run_profile_editor(config_path: &str, profile_path: Option<&str>) -> Result<()> {
    let (target_path, profile) = load_profile_for_editor(config_path, profile_path)?;
    let window = ProfileEditorWindow::new().context("failed to create Slint profile editor")?;
    let profile_state = Rc::new(RefCell::new(profile));
    let path = Rc::new(target_path);
    let diagnostics_log = Rc::new(DiagnosticsLog::new(DiagnosticsLog::default_path()?));
    let observability = Rc::new(RefCell::new(ObservabilityController::new(
        (*diagnostics_log).clone(),
    )));
    let settings_path = Rc::new(editor_settings_path()?);
    let (loaded_editor_settings, settings_notice) = load_editor_settings(settings_path.as_path());
    let editor_settings = Rc::new(RefCell::new(loaded_editor_settings));
    let device_catalog = Rc::new(DeviceCatalog::load());
    let bridge_controller = Rc::new(RefCell::new(BridgeController::new(
        config_path.to_string(),
        (*path).clone(),
        (*diagnostics_log).clone(),
    )));

    ProfileEditorState::from(&*profile_state.borrow()).apply_to_window(&window, path.as_path());
    editor_settings.borrow().apply_to_window(&window);
    window.set_steering_device_options(device_catalog.model());
    window.set_device_catalog_message(device_catalog.empty_message());
    window.set_status_message(SharedString::from(initial_status_message(
        settings_notice,
        device_catalog.as_ref(),
    )));
    refresh_runtime_summaries(&window, device_catalog.as_ref());
    sync_bridge_panel(&window, &bridge_controller.borrow());
    refresh_diagnostics_panel(&window, &observability.borrow());

    let weak_window = window.as_weak();
    window.window().on_close_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return CloseRequestResponse::HideWindow;
        };

        if window.get_close_to_taskbar_enabled() {
            window.window().set_minimized(true);
            window.set_status_message(SharedString::from("Window minimized to taskbar."));
            CloseRequestResponse::KeepWindowShown
        } else {
            CloseRequestResponse::HideWindow
        }
    });

    let weak_window = window.as_weak();
    let profile_state_handle = Rc::clone(&profile_state);
    let path_handle = Rc::clone(&path);
    let bridge_controller_handle = Rc::clone(&bridge_controller);
    let device_catalog_handle = Rc::clone(&device_catalog);
    window.on_save_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        match save_window_profile(&window, &profile_state_handle, path_handle.as_path()) {
            Ok(_) => {
                refresh_runtime_summaries(&window, device_catalog_handle.as_ref());

                let bridge_running = bridge_controller_handle.borrow().is_running();
                let message = if bridge_running {
                    if window.get_hot_reload_enabled() {
                        "Saved. Bridge will reload."
                    } else {
                        "Saved. Restart bridge to apply."
                    }
                } else {
                    "Profile saved."
                };
                window.set_status_message(SharedString::from(message));
            }
            Err(error) => {
                window.set_status_message(SharedString::from(format!("Save failed: {error}")));
            }
        }
    });

    let weak_window = window.as_weak();
    let profile_state_handle = Rc::clone(&profile_state);
    let path_handle = Rc::clone(&path);
    let bridge_controller_handle = Rc::clone(&bridge_controller);
    let device_catalog_handle = Rc::clone(&device_catalog);
    let observability_handle = Rc::clone(&observability);
    window.on_start_bridge_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        if let Err(error) =
            save_window_profile(&window, &profile_state_handle, path_handle.as_path())
        {
            window.set_status_message(SharedString::from(format!("Save failed: {error}")));
            return;
        }

        refresh_runtime_summaries(&window, device_catalog_handle.as_ref());

        let mut bridge_controller = bridge_controller_handle.borrow_mut();
        match bridge_controller.start(window.get_hot_reload_enabled()) {
            Ok(()) => {
                sync_bridge_panel(&window, &bridge_controller);
                refresh_diagnostics_panel(&window, &observability_handle.borrow());
                window.set_status_message(SharedString::from(if window.get_hot_reload_enabled() {
                    "Bridge started. Hot reload on."
                } else {
                    "Bridge started. Hot reload off."
                }));
            }
            Err(error) => {
                sync_bridge_panel(&window, &bridge_controller);
                refresh_diagnostics_panel(&window, &observability_handle.borrow());
                window.set_status_message(SharedString::from(format!(
                    "Bridge launch failed: {error}"
                )));
            }
        }
    });

    let weak_window = window.as_weak();
    let bridge_controller_handle = Rc::clone(&bridge_controller);
    let observability_handle = Rc::clone(&observability);
    window.on_stop_bridge_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        let mut bridge_controller = bridge_controller_handle.borrow_mut();
        match bridge_controller.stop() {
            Ok(()) => {
                sync_bridge_panel(&window, &bridge_controller);
                refresh_diagnostics_panel(&window, &observability_handle.borrow());
                window.set_status_message(SharedString::from("Bridge stopped."));
            }
            Err(error) => {
                sync_bridge_panel(&window, &bridge_controller);
                refresh_diagnostics_panel(&window, &observability_handle.borrow());
                window
                    .set_status_message(SharedString::from(format!("Bridge stop failed: {error}")));
            }
        }
    });

    let weak_window = window.as_weak();
    let device_catalog_handle = Rc::clone(&device_catalog);
    window.on_select_steering_device_requested(move |index| {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        match device_catalog_handle.device_id_for_index(index) {
            Some(device_id) => {
                window.set_steering_device(device_id as f32);
                refresh_runtime_summaries(&window, device_catalog_handle.as_ref());
                window.set_status_message(SharedString::from(format!(
                    "Selected {}.",
                    device_catalog_handle.label_for_device(Some(device_id))
                )));
            }
            None => {
                window.set_steering_device(-1.0);
                refresh_runtime_summaries(&window, device_catalog_handle.as_ref());
                window.set_status_message(SharedString::from("Steering input cleared."));
            }
        }
    });

    let weak_window = window.as_weak();
    let device_catalog_handle = Rc::clone(&device_catalog);
    window.on_clear_steering_device_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        window.set_steering_device(-1.0);
        refresh_runtime_summaries(&window, device_catalog_handle.as_ref());
        window.set_status_message(SharedString::from("Steering input cleared."));
    });

    let weak_window = window.as_weak();
    let observability_handle = Rc::clone(&observability);
    window.on_export_session_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        match observability_handle.borrow().export_live_session() {
            Ok(path) => {
                refresh_diagnostics_panel(&window, &observability_handle.borrow());
                window.set_status_message(SharedString::from(format!(
                    "Exported session to {}.",
                    path.display()
                )));
            }
            Err(error) => {
                refresh_diagnostics_panel(&window, &observability_handle.borrow());
                window.set_status_message(SharedString::from(format!(
                    "Session export failed: {error}"
                )));
            }
        }
    });

    let weak_window = window.as_weak();
    let observability_handle = Rc::clone(&observability);
    window.on_replay_latest_session_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        match observability_handle.borrow_mut().replay_latest_session() {
            Ok(path) => {
                refresh_diagnostics_panel(&window, &observability_handle.borrow());
                window.set_status_message(SharedString::from(format!(
                    "Replaying exported session {}.",
                    path.display()
                )));
            }
            Err(error) => {
                refresh_diagnostics_panel(&window, &observability_handle.borrow());
                window.set_status_message(SharedString::from(format!("Replay failed: {error}")));
            }
        }
    });

    let weak_window = window.as_weak();
    let observability_handle = Rc::clone(&observability);
    window.on_return_to_live_session_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        let returned = observability_handle.borrow_mut().return_to_live();
        refresh_diagnostics_panel(&window, &observability_handle.borrow());
        window.set_status_message(SharedString::from(if returned {
            "Returned to the live session log."
        } else {
            "Already viewing the live session log."
        }));
    });

    let weak_window = window.as_weak();
    let bridge_controller_handle = Rc::clone(&bridge_controller);
    let device_catalog_handle = Rc::clone(&device_catalog);
    let editor_settings_handle = Rc::clone(&editor_settings);
    let settings_path_handle = Rc::clone(&settings_path);
    window.on_toggle_hot_reload_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        let enabled = !window.get_hot_reload_enabled();
        window.set_hot_reload_enabled(enabled);

        if let Err(error) = save_window_editor_settings(
            &window,
            &editor_settings_handle,
            settings_path_handle.as_path(),
        ) {
            window.set_hot_reload_enabled(!enabled);
            refresh_runtime_summaries(&window, device_catalog_handle.as_ref());
            window.set_status_message(SharedString::from(format!(
                "Failed to save settings: {error}"
            )));
            return;
        }

        refresh_runtime_summaries(&window, device_catalog_handle.as_ref());

        let message = if bridge_controller_handle.borrow().is_running() {
            if enabled {
                "Hot reload on. Restart bridge."
            } else {
                "Hot reload off. Restart bridge."
            }
        } else if enabled {
            "Hot reload on."
        } else {
            "Hot reload off."
        };
        window.set_status_message(SharedString::from(message));
    });

    let weak_window = window.as_weak();
    let editor_settings_handle = Rc::clone(&editor_settings);
    let settings_path_handle = Rc::clone(&settings_path);
    window.on_toggle_close_to_taskbar_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        let enabled = !window.get_close_to_taskbar_enabled();
        window.set_close_to_taskbar_enabled(enabled);

        if let Err(error) = save_window_editor_settings(
            &window,
            &editor_settings_handle,
            settings_path_handle.as_path(),
        ) {
            window.set_close_to_taskbar_enabled(!enabled);
            window.set_status_message(SharedString::from(format!(
                "Failed to save settings: {error}"
            )));
            return;
        }

        window.set_status_message(SharedString::from(if enabled {
            "Close now minimizes to taskbar."
        } else {
            "Close now exits Torquebridge."
        }));
    });

    let weak_window = window.as_weak();
    let bridge_controller_handle = Rc::clone(&bridge_controller);
    let observability_handle = Rc::clone(&observability);
    window.on_quit_and_shutdown_requested(move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        let stop_result = {
            let mut bridge_controller = bridge_controller_handle.borrow_mut();
            let result = bridge_controller.stop();
            sync_bridge_panel(&window, &bridge_controller);
            result
        };

        match stop_result {
            Ok(()) => {
                refresh_diagnostics_panel(&window, &observability_handle.borrow());
                window.set_status_message(SharedString::from("Shutting down Torquebridge."));
                let _ = window.hide();
            }
            Err(error) => {
                refresh_diagnostics_panel(&window, &observability_handle.borrow());
                window.set_status_message(SharedString::from(format!("Shutdown failed: {error}")));
            }
        }
    });

    let bridge_poll_timer = Timer::default();
    let weak_window = window.as_weak();
    let bridge_controller_handle = Rc::clone(&bridge_controller);
    let observability_handle = Rc::clone(&observability);
    bridge_poll_timer.start(TimerMode::Repeated, Duration::from_millis(500), move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        let mut bridge_controller = bridge_controller_handle.borrow_mut();
        match bridge_controller.poll() {
            Ok(Some(message)) => {
                sync_bridge_panel(&window, &bridge_controller);
                refresh_diagnostics_panel(&window, &observability_handle.borrow());
                window.set_status_message(SharedString::from(message));
            }
            Ok(None) => {
                refresh_diagnostics_panel(&window, &observability_handle.borrow());
            }
            Err(error) => {
                sync_bridge_panel(&window, &bridge_controller);
                refresh_diagnostics_panel(&window, &observability_handle.borrow());
                window.set_status_message(SharedString::from(format!(
                    "Bridge runtime check failed: {error}"
                )));
            }
        }
    });

    let result = window.run().context("profile editor exited with an error");

    if let Err(error) = bridge_controller.borrow_mut().stop() {
        eprintln!("failed to stop bridge while closing editor: {error}");
    }

    result
}

fn load_profile_for_editor(
    config_path: &str,
    profile_path: Option<&str>,
) -> Result<(PathBuf, FfbProfile)> {
    let target_path = profile_path.map(PathBuf::from).unwrap_or(
        std::env::current_dir()
            .context("failed to resolve current working directory")?
            .join("profiles")
            .join("baseline.json"),
    );

    let profile = if target_path.exists() {
        load_profile(&target_path)?
    } else {
        let controllers = load_controllers(config_path)?;
        FfbProfile::from_ffb_settings(load_ffb_settings(&controllers)?)
    };

    Ok((target_path, profile))
}

fn load_ffb_settings(controllers: &[ControllerConfig]) -> Result<FfbParamsConfig> {
    controllers
        .iter()
        .find_map(|controller| controller.ffb_parameters.clone())
        .ok_or_else(|| anyhow!("no FFBParameters entry found in config"))
}

fn save_window_profile(
    window: &ProfileEditorWindow,
    profile_state: &RefCell<FfbProfile>,
    path: &Path,
) -> Result<FfbProfile> {
    let updated = {
        let current = profile_state.borrow();
        ProfileEditorState::from_window(window).into_profile(&current)
    };

    save_profile(path, &updated)?;
    *profile_state.borrow_mut() = updated.clone();
    Ok(updated)
}

fn refresh_runtime_summaries(window: &ProfileEditorWindow, device_catalog: &DeviceCatalog) {
    let steering_summary = device_catalog.label_for_device(selected_steering_device(window));

    window.set_runtime_poll_summary(SharedString::from(format!(
        "{} ms",
        window.get_poll_ms().round() as i32
    )));
    window.set_selected_steering_device_index(
        device_catalog.selected_index(selected_steering_device(window)),
    );
    window.set_selected_steering_device_label(SharedString::from(steering_summary.clone()));
    window.set_steering_device_summary(SharedString::from(steering_summary));
    window.set_hot_reload_summary(SharedString::from(hot_reload_summary(
        window.get_hot_reload_enabled(),
    )));
}

fn refresh_diagnostics_panel(
    window: &ProfileEditorWindow,
    observability: &ObservabilityController,
) {
    let source_label = observability.source_label();
    let diagnostics_log = observability.current_log();

    window.set_observability_source_label(SharedString::from(source_label.clone()));
    window.set_diagnostics_log_path(SharedString::from(
        diagnostics_log.path().display().to_string(),
    ));

    match diagnostics_log.read_recent(OBSERVABILITY_EVENT_LIMIT) {
        Ok(events) => {
            apply_diagnostics_view(window, &events);
            TelemetryPanelView::from_events(&events, &source_label).apply_to_window(window);
        }
        Err(error) => {
            apply_diagnostics_error(window, &error.to_string());
            TelemetryPanelView::failed(&source_label, &error.to_string()).apply_to_window(window);
        }
    }
}

fn apply_diagnostics_view(window: &ProfileEditorWindow, events: &[BridgeDiagnosticEvent]) {
    let filtered_events = events
        .iter()
        .filter(|event| event.category != DiagnosticCategory::Telemetry)
        .collect::<Vec<_>>();
    let diagnostics_events = if filtered_events.len() > DIAGNOSTIC_EVENT_LIMIT {
        &filtered_events[filtered_events.len() - DIAGNOSTIC_EVENT_LIMIT..]
    } else {
        &filtered_events
    };

    let diagnostics_text = if diagnostics_events.is_empty() {
        "No bridge diagnostics yet. Launch the bridge to capture startup, apply, telemetry, and safety events."
            .to_string()
    } else {
        diagnostics_events
            .iter()
            .rev()
            .map(|event| format_event(event))
            .collect::<Vec<_>>()
            .join("\n\n")
    };

    window.set_diagnostics_status(SharedString::from(if diagnostics_events.is_empty() {
        "Waiting for session"
    } else {
        "Tail updated"
    }));
    window.set_diagnostics_event_count(SharedString::from(format!(
        "{} recent event{}",
        diagnostics_events.len(),
        if diagnostics_events.len() == 1 {
            ""
        } else {
            "s"
        }
    )));
    window.set_diagnostics_events(SharedString::from(diagnostics_text));
}

fn apply_diagnostics_error(window: &ProfileEditorWindow, error: &str) {
    window.set_diagnostics_status(SharedString::from("Read failed"));
    window.set_diagnostics_event_count(SharedString::from("Diagnostics unavailable"));
    window.set_diagnostics_events(SharedString::from(format!(
        "Failed to read diagnostics log: {error}"
    )));
}

fn sync_bridge_panel(window: &ProfileEditorWindow, bridge_controller: &BridgeController) {
    window.set_bridge_running(bridge_controller.is_running());
    window.set_bridge_status(SharedString::from(bridge_controller.status_label()));
    window.set_bridge_detail(SharedString::from(bridge_controller.detail().to_string()));
}

fn apply_bridge_safety_reset(
    config_path: &str,
    diagnostics_log: &DiagnosticsLog,
    phase: &str,
) -> Result<String> {
    let result = (|| -> Result<String> {
        let controllers = load_controllers(config_path)?;
        let direct_input = DirectInput::create()?;
        let mut output = direct_input.open_configured_ffb_device(&controllers)?;
        output.apply_commands(&[
            WheelCommand::DeviceControl(DeviceControlCommand::StopAll),
            WheelCommand::DeviceControl(DeviceControlCommand::Reset),
        ])?;

        Ok(format!(
            "Safety reset sent to '{}'.",
            output.info().instance_name,
        ))
    })();

    match result {
        Ok(detail) => {
            record_ui_event(
                diagnostics_log,
                DiagnosticLevel::Info,
                DiagnosticCategory::Safety,
                format!("{phase} safety reset applied"),
                vec![DiagnosticField::new("detail", detail.clone())],
            );
            Ok(detail)
        }
        Err(error) => {
            record_ui_event(
                diagnostics_log,
                DiagnosticLevel::Error,
                DiagnosticCategory::Error,
                format!("{phase} safety reset failed"),
                vec![DiagnosticField::new("error", error.to_string())],
            );
            Err(error)
        }
    }
}

fn record_ui_event(
    diagnostics_log: &DiagnosticsLog,
    level: DiagnosticLevel,
    category: DiagnosticCategory,
    message: impl Into<String>,
    fields: Vec<DiagnosticField>,
) {
    if let Err(error) = diagnostics_log.append(level, category, message, fields) {
        eprintln!("failed to write UI diagnostics event: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConditionFfbConfig, ConstFfbConfig, PeriodicFfbConfig, VibrationConfig};

    fn device(id: u32, name: &str) -> WinmmDeviceInfo {
        WinmmDeviceInfo {
            id,
            name: name.to_string(),
            axis_count: 4,
            button_count: 8,
        }
    }

    fn profile() -> FfbProfile {
        FfbProfile {
            version: 1,
            name: Some("Baseline Compatibility Bridge".to_string()),
            notes: Some("Hot-reload tuned profile".to_string()),
            steering_device: Some(0),
            poll_ms: Some(5),
            ffb_parameters: FfbParamsConfig {
                r#const: ConstFfbConfig {
                    magnitude: 1.0,
                    maximum_force: 0.92,
                    minimum_force: 0.03,
                    filter_threshold: 0.18,
                    minimum_coefficient: 0.55,
                },
                sine: PeriodicFfbConfig {
                    magnitude: 0.75,
                    frequency: 1.0,
                    maximum_force: 0.75,
                    minimum_force: 0.0,
                    phase: 0.375,
                    engine_vibrations: VibrationConfig {
                        frequency: 1.0,
                        strength: 0.015,
                    },
                    gear_shift_vibrations: VibrationConfig {
                        frequency: 1.0,
                        strength: 0.06,
                    },
                },
                spring: ConditionFfbConfig {
                    coefficient: 0.03,
                    saturation: 0.0,
                },
                damper: ConditionFfbConfig {
                    coefficient: 0.02,
                    saturation: 0.0,
                },
            },
        }
    }

    #[test]
    fn editor_state_round_trips_profile_values() {
        let original = profile();
        let state = ProfileEditorState::from(&original);
        let restored = state.into_profile(&original);

        assert_eq!(restored.name, original.name);
        assert_eq!(restored.notes, original.notes);
        assert_eq!(restored.poll_ms, original.poll_ms);
        assert_eq!(restored.steering_device, original.steering_device);
        assert!((restored.ffb_parameters.r#const.maximum_force - 0.92).abs() < f32::EPSILON);
        assert!((restored.ffb_parameters.sine.phase - 0.375).abs() < f32::EPSILON);
        assert!((restored.ffb_parameters.spring.coefficient - 0.03).abs() < f32::EPSILON);
    }

    #[test]
    fn editor_state_clamps_out_of_range_values() {
        let original = profile();
        let mut state = ProfileEditorState::from(&original);
        state.poll_ms = 99.0;
        state.steering_device = -4.0;
        state.const_magnitude = 4.0;
        state.damper_saturation = -2.0;

        let restored = state.into_profile(&original);
        assert_eq!(restored.poll_ms, Some(20));
        assert_eq!(restored.steering_device, None);
        assert_eq!(restored.ffb_parameters.r#const.magnitude, 2.0);
        assert_eq!(restored.ffb_parameters.damper.saturation, 0.0);
    }

    #[test]
    fn device_catalog_uses_names_for_selected_device() {
        let catalog = DeviceCatalog {
            devices: vec![device(0, "FFBeast Wheel"), device(2, "Gamepad")],
            load_error: None,
        };

        assert_eq!(catalog.selected_index(None), 0);
        assert_eq!(catalog.selected_index(Some(2)), 2);
        assert_eq!(catalog.selected_index(Some(7)), 0);
        assert_eq!(catalog.device_id_for_index(0), None);
        assert_eq!(catalog.device_id_for_index(2), Some(2));
        assert_eq!(
            catalog.label_for_device(Some(0)),
            "FFBeast Wheel [WinMM #0]"
        );
        assert_eq!(catalog.label_for_device(None), "No steering device");
        assert_eq!(hot_reload_summary(true), "On save");
        assert_eq!(hot_reload_summary(false), "Manual");
    }

    #[test]
    fn editor_settings_round_trip_json() {
        let settings = EditorSettings {
            hot_reload_enabled: false,
            close_to_taskbar_enabled: true,
        };

        let json = serde_json::to_string(&settings).expect("serialize settings");
        let restored: EditorSettings = serde_json::from_str(&json).expect("deserialize settings");

        assert_eq!(restored, settings);
    }

    #[test]
    fn telemetry_panel_prefers_latest_telemetry_snapshot() {
        let older = BridgeDiagnosticEvent {
            timestamp_ms: 1,
            level: DiagnosticLevel::Info,
            category: DiagnosticCategory::Telemetry,
            message: "Runtime telemetry snapshot".to_string(),
            fields: vec![
                DiagnosticField::new("uptime", "0.5 s"),
                DiagnosticField::new("packet_rate", "1.0/s"),
                DiagnosticField::new("packet_rate_value", "1.0"),
                DiagnosticField::new("command_rate", "2.0/s"),
                DiagnosticField::new("command_rate_value", "2.0"),
                DiagnosticField::new("steering", "+0% (32768)"),
                DiagnosticField::new("peak_force", "18%"),
                DiagnosticField::new("peak_force_ratio", "0.1800"),
                DiagnosticField::new("clamp_status", "Constant has headroom at 18% peak."),
                DiagnosticField::new("saturation_status", "Spring saturation is 40%."),
                DiagnosticField::new("saturation_ratio", "0.4000"),
                DiagnosticField::new("last_packet_age", "500 ms ago"),
                DiagnosticField::new("input", "Older Input"),
                DiagnosticField::new("output", "Older Output"),
                DiagnosticField::new("poll_ms", "5 ms"),
                DiagnosticField::new("hot_reload", "On save"),
                DiagnosticField::new("updates_total", "1"),
                DiagnosticField::new("commands_total", "2"),
                DiagnosticField::new("last_update", "old update"),
                DiagnosticField::new("last_commands", "old commands"),
            ],
        };
        let latest = BridgeDiagnosticEvent {
            timestamp_ms: 2,
            level: DiagnosticLevel::Info,
            category: DiagnosticCategory::Telemetry,
            message: "Runtime telemetry snapshot".to_string(),
            fields: vec![
                DiagnosticField::new("uptime", "2.0 s"),
                DiagnosticField::new("packet_rate", "8.0/s"),
                DiagnosticField::new("packet_rate_value", "8.0"),
                DiagnosticField::new("command_rate", "16.0/s"),
                DiagnosticField::new("command_rate_value", "16.0"),
                DiagnosticField::new("steering", "+12% (36600)"),
                DiagnosticField::new("peak_force", "92%"),
                DiagnosticField::new("peak_force_ratio", "0.9200"),
                DiagnosticField::new("clamp_status", "Periodic is near clamp at 92%."),
                DiagnosticField::new("saturation_status", "Spring saturation is capped at 100%."),
                DiagnosticField::new("saturation_ratio", "1.0000"),
                DiagnosticField::new("last_packet_age", "80 ms ago"),
                DiagnosticField::new("input", "FFBeast Wheel [WinMM #0]"),
                DiagnosticField::new("output", "FFBeast Racing Wheel"),
                DiagnosticField::new("poll_ms", "4 ms"),
                DiagnosticField::new("hot_reload", "Manual"),
                DiagnosticField::new("updates_total", "12"),
                DiagnosticField::new("commands_total", "24"),
                DiagnosticField::new("last_update", "constant(magnitude=4000)"),
                DiagnosticField::new("last_commands", "constant(magnitude=-2500)"),
            ],
        };

        let view = TelemetryPanelView::from_events(
            &[
                BridgeDiagnosticEvent {
                    timestamp_ms: 0,
                    level: DiagnosticLevel::Info,
                    category: DiagnosticCategory::Lifecycle,
                    message: "Bridge launched".to_string(),
                    fields: Vec::new(),
                },
                older,
                latest,
            ],
            "Source: live session log",
        );

        assert_eq!(view.uptime, "2.0 s");
        assert_eq!(view.packet_rate, "8.0/s");
        assert_eq!(view.command_rate, "16.0/s");
        assert_eq!(view.steering, "+12% (36600)");
        assert_eq!(view.peak_force, "92%");
        assert!(view.status.contains("80 ms ago"));
        assert!(view.status.contains("Source: live session log"));
        assert_eq!(view.clamp_status, "Periodic is near clamp at 92%.");
        assert_eq!(
            view.saturation_status,
            "Spring saturation is capped at 100%."
        );
        assert!(view.runtime_detail.contains("FFBeast Wheel [WinMM #0]"));
        assert!(view.runtime_detail.contains("FFBeast Racing Wheel"));
        assert!(view.runtime_detail.contains("4 ms"));
        assert!(view.runtime_detail.contains("Manual"));
        assert!(view.runtime_detail.contains("12 updates / 24 commands"));
        assert!(view.packet_trend.contains("peak 8.0/s"));
        assert!(view.command_trend.contains("peak 16.0/s"));
        assert!(view.force_trend.contains("peak 92%"));
        assert_eq!(view.last_update, "constant(magnitude=4000)");
        assert_eq!(view.last_commands, "constant(magnitude=-2500)");
    }
}
