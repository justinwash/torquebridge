use anyhow::{Result, anyhow};
use clap::{Parser, Subcommand};
use std::thread;
use std::time::{Duration, Instant};
use torquebridge::backends::directinput::DirectInput;
use torquebridge::backends::ffbeast::{DirectControl, FFBeastBackend};
use torquebridge::config::{ControllerConfig, FfbParamsConfig, load_controllers};
use torquebridge::core::domain::{
    ConditionCommand, EffectMetadata, EffectUpdate, GameEffect, PeriodicCommand, WheelCommand,
};
use torquebridge::diagnostics::{
    DiagnosticCategory, DiagnosticField, DiagnosticLevel, DiagnosticsLog,
};
use torquebridge::effect_engine::{EffectEngine, steering_filter_coefficient};
use torquebridge::frontends::forza_vjoy::{
    InputFrame, InputMapper, RegisteredFfbCallback, VJoyDevice,
};
use torquebridge::inputs::WinmmJoystick;
use torquebridge::profile::{FfbProfile, ProfileWatcher, load_profile};
use torquebridge::ui::run_profile_editor;
#[cfg(windows)]
use winapi::um::wincon::{GetConsoleProcessList, GetConsoleWindow};
#[cfg(windows)]
use winapi::um::winuser::{SW_HIDE, ShowWindow};

#[derive(Debug, Parser)]
#[command(
    name = "Torquebridge",
    about = "Native FFBeast HID probe for replacing vJoy/EmuWheel"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Probe,
    ListInputs,
    ProbeInput {
        #[arg(long)]
        id: u32,
        #[arg(long, default_value_t = 20)]
        count: u32,
        #[arg(long, default_value_t = 100)]
        poll_ms: u64,
    },
    ListDirectInput {
        #[arg(long, default_value = "C:/Users/justi/Torquebridge/configuration.json")]
        config: String,
    },
    OpenFfbDevice {
        #[arg(long, default_value = "C:/Users/justi/Torquebridge/configuration.json")]
        config: String,
    },
    VJoyInit {
        #[arg(long, default_value_t = 1)]
        id: u32,
    },
    ShowConfig {
        #[arg(long, default_value = "C:/Users/justi/Torquebridge/configuration.json")]
        config: String,
    },
    ShowProfile {
        #[arg(long, default_value = "C:/Users/justi/Torquebridge/configuration.json")]
        config: String,
        #[arg(long)]
        profile: Option<String>,
    },
    Ui {
        #[arg(long)]
        config: Option<String>,
        #[arg(long)]
        profile: Option<String>,
    },
    FeederDemo {
        #[arg(long, default_value = "C:/Users/justi/Torquebridge/configuration.json")]
        config: String,
        #[arg(long, default_value_t = 1)]
        id: u32,
    },
    TranslateDemo {
        #[arg(long, default_value = "C:/Users/justi/Torquebridge/configuration.json")]
        config: String,
        #[arg(long)]
        profile: Option<String>,
    },
    FfbBridge {
        #[arg(long, default_value = "C:/Users/justi/Torquebridge/configuration.json")]
        config: String,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        no_profile_watch: bool,
        #[arg(long, default_value_t = 1)]
        id: u32,
        #[arg(long)]
        steering_device: Option<u32>,
        #[arg(long)]
        poll_ms: Option<u64>,
        #[arg(long, hide = true)]
        diagnostics_log: Option<String>,
    },
    State {
        #[arg(long, default_value_t = 10)]
        count: u32,
        #[arg(long, default_value_t = 50)]
        timeout_ms: i32,
    },
    Direct {
        #[arg(long, default_value_t = 0)]
        spring: i16,
        #[arg(long, default_value_t = 0)]
        constant: i16,
        #[arg(long, default_value_t = 0)]
        periodic: i16,
        #[arg(long, default_value_t = 0)]
        drop: u8,
        #[arg(long, default_value_t = 0)]
        hold_ms: u64,
    },
    Gain {
        #[arg(long)]
        percent: u8,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let command = cli.cmd.unwrap_or(Command::Ui {
        config: None,
        profile: None,
    });

    maybe_hide_console_for_ui_launch(&command);

    match command {
        Command::Probe => {
            let _backend = FFBeastBackend::connect()?;
            println!("Connected to FFBeast (045B:59D7)");
        }
        Command::ListInputs => {
            let devices = WinmmJoystick::list_devices()?;
            if devices.is_empty() {
                println!("no WinMM joystick devices found");
            } else {
                for device in devices {
                    println!(
                        "id={} name={} axes={} buttons={}",
                        device.id, device.name, device.axis_count, device.button_count
                    );
                }
            }
        }
        Command::ProbeInput { id, count, poll_ms } => {
            let mut input = WinmmJoystick::open(id)?;
            println!(
                "probing WinMM input device {} ({}) for {} sample(s) every {} ms",
                input.id(),
                input.name(),
                count,
                poll_ms
            );

            for sample in 0..count {
                let frame = input.poll_input_frame()?;
                println!(
                    "sample={} x={} y={} z={} rx={} ry={} rz={} pov={} buttons_down={}",
                    sample,
                    frame.x,
                    frame.y,
                    frame.z,
                    frame.rotation_x,
                    frame.rotation_y,
                    frame.rotation_z,
                    frame
                        .point_of_view_controllers
                        .first()
                        .copied()
                        .unwrap_or(-1),
                    frame.buttons.iter().filter(|pressed| **pressed).count(),
                );

                if sample + 1 < count {
                    thread::sleep(Duration::from_millis(poll_ms));
                }
            }
        }
        Command::ListDirectInput { config } => {
            let controllers = load_controllers(&config)?;
            let direct_input = DirectInput::create()?;
            let devices = direct_input.list_devices(&controllers)?;
            if devices.is_empty() {
                println!("no non-vJoy DirectInput game controllers found");
            } else {
                for device in devices {
                    println!(
                        "instance={} instance_guid={} product_guid={} configured={} configured_ffb={}",
                        device.instance_name,
                        device.instance_guid,
                        device.product_guid,
                        device.configured,
                        device.configured_ffb,
                    );
                }
            }
        }
        Command::OpenFfbDevice { config } => {
            let controllers = load_controllers(&config)?;
            let direct_input = DirectInput::create()?;
            let device = direct_input.open_configured_ffb_device(&controllers)?;
            let info = device.info();
            let caps = device.cached_capabilities();
            println!(
                "opened DirectInput FFB device '{}' instance_guid={} product_guid={} force_feedback_capable={}",
                info.instance_name,
                info.instance_guid,
                info.product_guid,
                caps.force_feedback_capable,
            );
        }
        Command::VJoyInit { id } => {
            let dev = VJoyDevice::initialize(id)?;
            println!(
                "vJoy device {} ready; ffb_capable={}",
                dev.id(),
                dev.is_ffb_capable()
            );
        }
        Command::ShowConfig { config } => {
            let controllers = load_controllers(&config)?;
            let mapper = InputMapper::from_config(&controllers);
            println!("loaded {} configured controller(s)", controllers.len());
            println!("steering mapped: {}", mapper.mapping.steering.is_some());
            println!("combined mapped: {}", mapper.mapping.combined.is_some());
            println!("buttons mapped: {}", mapper.mapping.buttons.len());
            println!("dpad mapped: {}", mapper.mapping.dpad.is_some());

            let ffb_device = controllers
                .iter()
                .find(|controller| controller.ffb_parameters.is_some());
            println!(
                "ffb device: {}",
                ffb_device
                    .map(|controller| controller.instance_name.as_str())
                    .unwrap_or("<none>")
            );
        }
        Command::ShowProfile { config, profile } => {
            let (_, effective_profile) = load_effective_profile(&config, profile.as_deref())?;
            println!("{}", serde_json::to_string_pretty(&effective_profile)?);
        }
        Command::Ui { config, profile } => {
            run_profile_editor(config.as_deref(), profile.as_deref())?;
        }
        Command::FeederDemo { config, id } => {
            let controllers = load_controllers(&config)?;
            let mut mapper = InputMapper::from_config(&controllers);
            let mut vjoy = VJoyDevice::initialize(id)?;
            mapper.reset_vjoy_state(&mut vjoy);

            let frames: Vec<InputFrame> = controllers
                .iter()
                .enumerate()
                .map(|(index, controller)| InputFrame {
                    x: if index == 0 { 40_000 } else { 32_767 },
                    y: 32_767,
                    z: 32_767,
                    rotation_x: 32_767,
                    rotation_y: 32_767,
                    rotation_z: 32_767,
                    sliders: [32_767, 32_767],
                    buttons: vec![
                        false;
                        controller
                            .buttons
                            .as_ref()
                            .map(|buttons| buttons.len().max(32))
                            .unwrap_or(32)
                    ],
                    point_of_view_controllers: vec![
                        -1;
                        controller
                            .d_pad
                            .as_ref()
                            .map(|dpad| dpad.index + 1)
                            .unwrap_or(1)
                    ],
                })
                .collect();

            mapper.apply_input_frames(&frames, &mut vjoy)?;
            println!(
                "feeder demo applied using {} input frame(s) to vJoy device {}",
                frames.len(),
                id
            );
        }
        Command::TranslateDemo { config, profile } => {
            let (_, effective_profile) = load_effective_profile(&config, profile.as_deref())?;
            let mut engine = EffectEngine::new(effective_profile.ffb_parameters.clone());

            let update = EffectUpdate::Apply(GameEffect::Constant {
                metadata: EffectMetadata {
                    duration_ms: 100,
                    gain: 10_000,
                    raw_gain: 255,
                    direction: 0,
                    sample_period: 0,
                    trigger_button: -1,
                    trigger_repeat_interval: 0,
                },
                magnitude: 8_000,
            });

            let commands = engine.translate(&update, -1);
            println!(
                "profile: {}",
                effective_profile
                    .name
                    .as_deref()
                    .unwrap_or("<embedded config>")
            );
            println!("translated commands: {:?}", commands);
        }
        Command::FfbBridge {
            config,
            profile,
            no_profile_watch,
            id,
            steering_device,
            poll_ms,
            diagnostics_log,
        } => {
            let (controllers, effective_profile) =
                load_effective_profile(&config, profile.as_deref())?;
            let settings = effective_profile.ffb_parameters.clone();
            let mut active_settings = settings.clone();
            let cli_steering_device = steering_device;
            let cli_poll_ms = poll_ms;
            let mut steering_device =
                effective_profile.resolved_steering_device(cli_steering_device);
            let mut poll_ms = effective_profile.resolved_poll_ms(cli_poll_ms, 5);
            let diagnostics_log = diagnostics_log.map(DiagnosticsLog::new);
            let mut profile_watcher = if no_profile_watch {
                None
            } else {
                profile.as_deref().map(ProfileWatcher::new).transpose()?
            };

            let vjoy = VJoyDevice::initialize(id)?;
            if !vjoy.is_ffb_capable() {
                return Err(anyhow!("vJoy device {id} is not FFB-capable"));
            }

            let callback = RegisteredFfbCallback::register(vjoy, settings)?;
            let direct_input = DirectInput::create()?;
            let mut output = direct_input.open_configured_ffb_device(&controllers)?;
            let mut steering_input = steering_device.map(WinmmJoystick::open).transpose()?;
            let actuator_ids = output
                .actuator_object_ids()
                .into_iter()
                .map(|value| format!("0x{value:08X}"))
                .collect::<Vec<_>>()
                .join(", ");
            let profile_name = effective_profile
                .name
                .as_deref()
                .unwrap_or("<embedded config>")
                .to_string();
            let mut telemetry = BridgeTelemetryState::new(
                poll_ms,
                profile_watcher.is_some(),
                steering_input_label(steering_input.as_ref()),
                output.info().instance_name.clone(),
            );
            telemetry.set_calibration(&active_settings);
            telemetry.refresh_filter_coefficient(&active_settings);

            println!(
                "registered FFB bridge on vJoy device {}; forwarding translated output to DirectInput device '{}' every {} ms",
                callback.id(),
                output.info().instance_name,
                poll_ms
            );

            println!(
                "using force profile {}",
                effective_profile
                    .name
                    .as_deref()
                    .unwrap_or("<embedded config>")
            );

            log_bridge_event(
                diagnostics_log.as_ref(),
                DiagnosticLevel::Info,
                DiagnosticCategory::Startup,
                "Bridge startup snapshot",
                vec![
                    DiagnosticField::new("vjoy_id", callback.id().to_string()),
                    DiagnosticField::new("ffb_capable", callback.is_ffb_capable().to_string()),
                    DiagnosticField::new("profile", profile_name.clone()),
                    DiagnosticField::new("poll_ms", poll_ms.to_string()),
                    DiagnosticField::new("output_device", output.info().instance_name.clone()),
                    DiagnosticField::new("output_guid", output.info().instance_guid.clone()),
                    DiagnosticField::new("actuator_count", output.actuator_count().to_string()),
                    DiagnosticField::new(
                        "actuator_ids",
                        if actuator_ids.is_empty() {
                            "<none>".to_string()
                        } else {
                            actuator_ids.clone()
                        },
                    ),
                ],
            );

            if let Some(watcher) = profile_watcher.as_ref() {
                println!(
                    "watching profile file {} for changes",
                    watcher.path().display()
                );
                log_bridge_event(
                    diagnostics_log.as_ref(),
                    DiagnosticLevel::Info,
                    DiagnosticCategory::Profile,
                    "Profile hot reload enabled",
                    vec![DiagnosticField::new(
                        "path",
                        watcher.path().display().to_string(),
                    )],
                );
            } else if profile.is_some() && no_profile_watch {
                println!("profile hot reload disabled for this bridge session");
                log_bridge_event(
                    diagnostics_log.as_ref(),
                    DiagnosticLevel::Info,
                    DiagnosticCategory::Profile,
                    "Profile hot reload disabled",
                    Vec::new(),
                );
            }

            let steering_status = steering_input_status_message(steering_input.as_ref());
            println!("{steering_status}");
            log_bridge_event(
                diagnostics_log.as_ref(),
                DiagnosticLevel::Info,
                DiagnosticCategory::Input,
                steering_status,
                steering_input
                    .as_ref()
                    .map(|input| {
                        vec![
                            DiagnosticField::new("device_id", input.id().to_string()),
                            DiagnosticField::new("device_name", input.name().to_string()),
                        ]
                    })
                    .unwrap_or_default(),
            );
            if let Some(fields) = telemetry.snapshot_if_due() {
                log_bridge_event(
                    diagnostics_log.as_ref(),
                    DiagnosticLevel::Info,
                    DiagnosticCategory::Telemetry,
                    "Runtime telemetry snapshot",
                    fields,
                );
            }

            let mut last_update_signature: Option<String> = None;

            loop {
                if let Some(watcher) = profile_watcher.as_mut() {
                    match watcher.reload_if_changed() {
                        Ok(Some(reloaded_profile)) => {
                            callback.replace_settings(reloaded_profile.ffb_parameters.clone())?;
                            active_settings = reloaded_profile.ffb_parameters.clone();
                            telemetry.set_calibration(&active_settings);
                            telemetry.refresh_filter_coefficient(&active_settings);

                            let next_poll_ms = reloaded_profile.resolved_poll_ms(cli_poll_ms, 5);
                            if next_poll_ms != poll_ms {
                                poll_ms = next_poll_ms;
                                telemetry.set_poll_ms(poll_ms);
                                println!("updated profile poll interval to {} ms", poll_ms);
                                log_bridge_event(
                                    diagnostics_log.as_ref(),
                                    DiagnosticLevel::Info,
                                    DiagnosticCategory::Profile,
                                    "Profile changed bridge poll interval",
                                    vec![DiagnosticField::new("poll_ms", poll_ms.to_string())],
                                );
                            }

                            let next_steering_device =
                                reloaded_profile.resolved_steering_device(cli_steering_device);
                            if next_steering_device != steering_device {
                                steering_input =
                                    next_steering_device.map(WinmmJoystick::open).transpose()?;
                                steering_device = next_steering_device;
                                telemetry.set_steering_input_label(steering_input_label(
                                    steering_input.as_ref(),
                                ));
                                if steering_input.is_none() {
                                    telemetry.set_steering_state(None);
                                }
                                let steering_status =
                                    steering_input_status_message(steering_input.as_ref());
                                println!("{steering_status}");
                                log_bridge_event(
                                    diagnostics_log.as_ref(),
                                    DiagnosticLevel::Info,
                                    DiagnosticCategory::Input,
                                    steering_status,
                                    steering_input
                                        .as_ref()
                                        .map(|input| {
                                            vec![
                                                DiagnosticField::new(
                                                    "device_id",
                                                    input.id().to_string(),
                                                ),
                                                DiagnosticField::new(
                                                    "device_name",
                                                    input.name().to_string(),
                                                ),
                                            ]
                                        })
                                        .unwrap_or_default(),
                                );
                            }

                            println!(
                                "reloaded force profile {}",
                                reloaded_profile.name.as_deref().unwrap_or("<unnamed>")
                            );
                            log_bridge_event(
                                diagnostics_log.as_ref(),
                                DiagnosticLevel::Info,
                                DiagnosticCategory::Profile,
                                "Reloaded force profile",
                                vec![DiagnosticField::new(
                                    "profile",
                                    reloaded_profile
                                        .name
                                        .as_deref()
                                        .unwrap_or("<unnamed>")
                                        .to_string(),
                                )],
                            );
                        }
                        Ok(None) => {}
                        Err(error) => {
                            eprintln!(
                                "profile reload failed for {}: {}",
                                watcher.path().display(),
                                error
                            );
                            log_bridge_event(
                                diagnostics_log.as_ref(),
                                DiagnosticLevel::Error,
                                DiagnosticCategory::Error,
                                "Profile reload failed",
                                vec![
                                    DiagnosticField::new(
                                        "path",
                                        watcher.path().display().to_string(),
                                    ),
                                    DiagnosticField::new("error", error.to_string()),
                                ],
                            );
                        }
                    }
                }

                if let Some(input) = steering_input.as_mut() {
                    let frame = input.poll_input_frame()?;
                    callback.update_steering_state(frame.x)?;
                    telemetry.set_steering_state(Some(frame.x));
                    telemetry.refresh_filter_coefficient(&active_settings);
                }

                if let Some(update) = callback.take_pending_update()? {
                    let update_summary = summarize_effect_updates(&update.updates);
                    let command_summary = summarize_wheel_commands(&update.commands);
                    let signature = format!("{update_summary} => {command_summary}");
                    let emit_diagnostics = last_update_signature.as_deref() != Some(&signature);
                    telemetry.record_update(&update_summary, &command_summary, &update.commands);

                    if emit_diagnostics {
                        log_bridge_event(
                            diagnostics_log.as_ref(),
                            DiagnosticLevel::Info,
                            DiagnosticCategory::Packet,
                            "Received translated FFB update",
                            vec![
                                DiagnosticField::new("updates", update_summary.clone()),
                                DiagnosticField::new("commands", command_summary.clone()),
                            ],
                        );
                    }

                    output.apply_commands(&update.commands)?;

                    if emit_diagnostics {
                        log_bridge_event(
                            diagnostics_log.as_ref(),
                            DiagnosticLevel::Info,
                            DiagnosticCategory::Apply,
                            "Applied wheel commands",
                            vec![
                                DiagnosticField::new(
                                    "command_count",
                                    update.commands.len().to_string(),
                                ),
                                DiagnosticField::new("commands", command_summary),
                            ],
                        );
                        last_update_signature = Some(signature);
                    }
                }

                if let Some(fields) = telemetry.snapshot_if_due() {
                    log_bridge_event(
                        diagnostics_log.as_ref(),
                        DiagnosticLevel::Info,
                        DiagnosticCategory::Telemetry,
                        "Runtime telemetry snapshot",
                        fields,
                    );
                }

                thread::sleep(Duration::from_millis(poll_ms));
            }
        }
        Command::State { count, timeout_ms } => {
            let backend = FFBeastBackend::connect()?;
            for _ in 0..count {
                let state = backend.read_state_blocking(timeout_ms)?;
                println!(
                    "fw={:?} reg={} pos={} ({:.3}) torque={} ({:.3})",
                    state.firmware_raw,
                    state.is_registered,
                    state.position_raw,
                    state.position_norm(),
                    state.torque_raw,
                    state.torque_norm()
                );
            }
        }
        Command::Direct {
            spring,
            constant,
            periodic,
            drop,
            hold_ms,
        } => {
            let backend = FFBeastBackend::connect()?;
            let command = DirectControl {
                spring_force: spring,
                constant_force: constant,
                periodic_force: periodic,
                force_drop: drop,
            };

            backend.send_direct_control(command)?;
            println!("Applied direct control: {:?}", command.clamped());

            if hold_ms > 0 {
                thread::sleep(Duration::from_millis(hold_ms));
                backend.send_direct_control(DirectControl {
                    spring_force: 0,
                    constant_force: 0,
                    periodic_force: 0,
                    force_drop: 0,
                })?;
                println!("Returned force outputs to zero");
            }
        }
        Command::Gain { percent } => {
            let backend = FFBeastBackend::connect()?;
            backend.set_device_gain(percent)?;
            println!("Set device gain to {}%", percent.min(100));
        }
    }

    Ok(())
}

#[cfg(windows)]
fn maybe_hide_console_for_ui_launch(command: &Command) {
    if !matches!(command, Command::Ui { .. }) {
        return;
    }

    unsafe {
        // Only hide if this process owns the console (typical when launched from Explorer).
        let mut process_list = [0u32; 2];
        let attached_count =
            GetConsoleProcessList(process_list.as_mut_ptr(), process_list.len() as u32);
        if attached_count <= 1 {
            let console_window = GetConsoleWindow();
            if !console_window.is_null() {
                ShowWindow(console_window, SW_HIDE);
            }
        }
    }
}

#[cfg(not(windows))]
fn maybe_hide_console_for_ui_launch(_command: &Command) {}

fn load_effective_profile(
    config: &str,
    profile_path: Option<&str>,
) -> Result<(Vec<ControllerConfig>, FfbProfile)> {
    if let Some(path) = profile_path {
        let profile = load_profile(path)?;
        let controllers = match profile
            .runtime_controllers
            .clone()
            .filter(|controllers| !controllers.is_empty())
        {
            Some(controllers) => controllers,
            None => load_controllers(config)?,
        };
        return Ok((controllers, profile));
    }

    let controllers = load_controllers(config)?;
    let profile = FfbProfile::from_ffb_settings(load_ffb_settings(&controllers)?);
    Ok((controllers, profile))
}

fn load_ffb_settings(controllers: &[ControllerConfig]) -> Result<FfbParamsConfig> {
    controllers
        .iter()
        .find_map(|controller| controller.ffb_parameters.clone())
        .ok_or_else(|| anyhow!("no FFBParameters entry found in config"))
}

fn steering_input_status_message(input: Option<&WinmmJoystick>) -> String {
    if let Some(input) = input {
        format!(
            "using WinMM input device {} ({}) for live steering-state updates",
            input.id(),
            input.name()
        )
    } else {
        "no live steering input selected; run 'Torquebridge list-inputs' and pass --steering-device <id> to enable steering-state updates"
            .to_string()
    }
}

fn log_bridge_event(
    diagnostics_log: Option<&DiagnosticsLog>,
    level: DiagnosticLevel,
    category: DiagnosticCategory,
    message: impl Into<String>,
    fields: Vec<DiagnosticField>,
) {
    let Some(diagnostics_log) = diagnostics_log else {
        return;
    };

    if let Err(error) = diagnostics_log.append(level, category, message, fields) {
        eprintln!("failed to write bridge diagnostics event: {error}");
    }
}

const TELEMETRY_INTERVAL: Duration = Duration::from_millis(500);

struct BridgeTelemetryState {
    started_at: Instant,
    last_snapshot_at: Instant,
    last_update_at: Option<Instant>,
    packet_updates_total: u64,
    packet_updates_since_snapshot: u64,
    applied_commands_total: u64,
    applied_commands_since_snapshot: u64,
    steering_state: Option<i32>,
    steering_input_label: String,
    output_device_name: String,
    poll_ms: u64,
    hot_reload_enabled: bool,
    calibration_preset: String,
    calibration_output_gain: f32,
    calibration_const_gain: f32,
    calibration_sine_gain: f32,
    calibration_spring_gain: f32,
    calibration_damper_gain: f32,
    calibration_center_offset: f32,
    calibration_range: f32,
    calibration_curve: f32,
    steering_filter_coefficient: Option<f32>,
    peak_force_ratio: f32,
    condition_saturation_ratio: f32,
    peak_force_label: String,
    clamp_status: String,
    saturation_status: String,
    last_update_summary: String,
    last_command_summary: String,
}

impl BridgeTelemetryState {
    fn new(
        poll_ms: u64,
        hot_reload_enabled: bool,
        steering_input_label: String,
        output_device_name: String,
    ) -> Self {
        let now = Instant::now();

        Self {
            started_at: now,
            last_snapshot_at: now.checked_sub(TELEMETRY_INTERVAL).unwrap_or(now),
            last_update_at: None,
            packet_updates_total: 0,
            packet_updates_since_snapshot: 0,
            applied_commands_total: 0,
            applied_commands_since_snapshot: 0,
            steering_state: None,
            steering_input_label,
            output_device_name,
            poll_ms,
            hot_reload_enabled,
            calibration_preset: "Custom".to_string(),
            calibration_output_gain: 1.0,
            calibration_const_gain: 1.0,
            calibration_sine_gain: 1.0,
            calibration_spring_gain: 1.0,
            calibration_damper_gain: 1.0,
            calibration_center_offset: 0.0,
            calibration_range: 1.0,
            calibration_curve: 1.0,
            steering_filter_coefficient: None,
            peak_force_ratio: 0.0,
            condition_saturation_ratio: 0.0,
            peak_force_label: "0%".to_string(),
            clamp_status: "No active force output.".to_string(),
            saturation_status: "No condition saturation activity.".to_string(),
            last_update_summary: "Waiting for translated packet updates.".to_string(),
            last_command_summary: "Waiting for applied wheel commands.".to_string(),
        }
    }

    fn set_poll_ms(&mut self, poll_ms: u64) {
        self.poll_ms = poll_ms;
    }

    fn set_steering_input_label(&mut self, steering_input_label: String) {
        self.steering_input_label = steering_input_label;
    }

    fn set_steering_state(&mut self, steering_state: Option<i32>) {
        self.steering_state = steering_state;
    }

    fn set_calibration(&mut self, settings: &FfbParamsConfig) {
        self.calibration_preset = settings
            .calibration
            .preset
            .clone()
            .unwrap_or_else(|| "Custom".to_string());
        self.calibration_output_gain = settings.calibration.output_gain.clamp(0.0, 2.0);
        self.calibration_const_gain = settings.calibration.const_gain.clamp(0.0, 2.0);
        self.calibration_sine_gain = settings.calibration.sine_gain.clamp(0.0, 2.0);
        self.calibration_spring_gain = settings.calibration.spring_gain.clamp(0.0, 2.0);
        self.calibration_damper_gain = settings.calibration.damper_gain.clamp(0.0, 2.0);
        self.calibration_center_offset =
            settings.calibration.steering_center_offset.clamp(-0.5, 0.5);
        self.calibration_range = settings.calibration.steering_range.clamp(0.25, 2.0);
        self.calibration_curve = settings.calibration.steering_curve.clamp(0.25, 3.0);
    }

    fn refresh_filter_coefficient(&mut self, settings: &FfbParamsConfig) {
        self.steering_filter_coefficient = self.steering_state.map(|state| {
            steering_filter_coefficient(settings, state, settings.r#const.minimum_coefficient)
        });
    }

    fn record_update(
        &mut self,
        update_summary: &str,
        command_summary: &str,
        commands: &[WheelCommand],
    ) {
        let now = Instant::now();
        self.last_update_at = Some(now);
        self.packet_updates_total += 1;
        self.packet_updates_since_snapshot += 1;
        self.applied_commands_total += commands.len() as u64;
        self.applied_commands_since_snapshot += commands.len() as u64;
        let force_metrics = analyze_wheel_commands(commands);
        self.peak_force_ratio = force_metrics.peak_force_ratio;
        self.condition_saturation_ratio = force_metrics.condition_saturation_ratio;
        self.peak_force_label = force_metrics.peak_force_label;
        self.clamp_status = force_metrics.clamp_status;
        self.saturation_status = force_metrics.saturation_status;
        self.last_update_summary = update_summary.to_string();
        self.last_command_summary = command_summary.to_string();
    }

    fn snapshot_fields(&self) -> Vec<DiagnosticField> {
        let now = Instant::now();
        let interval = now.saturating_duration_since(self.last_snapshot_at);
        let interval_seconds = interval.as_secs_f32().max(0.001);
        let packet_rate = self.packet_updates_since_snapshot as f32 / interval_seconds;
        let command_rate = self.applied_commands_since_snapshot as f32 / interval_seconds;

        vec![
            DiagnosticField::new(
                "uptime",
                format_elapsed(now.saturating_duration_since(self.started_at)),
            ),
            DiagnosticField::new("packet_rate", format!("{packet_rate:.1}/s")),
            DiagnosticField::new("packet_rate_value", format!("{packet_rate:.3}")),
            DiagnosticField::new("command_rate", format!("{command_rate:.1}/s")),
            DiagnosticField::new("command_rate_value", format!("{command_rate:.3}")),
            DiagnosticField::new("steering", format_steering_state(self.steering_state)),
            DiagnosticField::new("peak_force", self.peak_force_label.clone()),
            DiagnosticField::new("peak_force_ratio", format!("{:.4}", self.peak_force_ratio)),
            DiagnosticField::new("clamp_status", self.clamp_status.clone()),
            DiagnosticField::new("saturation_status", self.saturation_status.clone()),
            DiagnosticField::new(
                "saturation_ratio",
                format!("{:.4}", self.condition_saturation_ratio),
            ),
            DiagnosticField::new(
                "last_packet_age",
                self.last_update_at
                    .map(|timestamp| {
                        format!(
                            "{} ago",
                            format_elapsed(now.saturating_duration_since(timestamp))
                        )
                    })
                    .unwrap_or_else(|| "No packet yet".to_string()),
            ),
            DiagnosticField::new("last_update", self.last_update_summary.clone()),
            DiagnosticField::new("last_commands", self.last_command_summary.clone()),
            DiagnosticField::new("input", self.steering_input_label.clone()),
            DiagnosticField::new("output", self.output_device_name.clone()),
            DiagnosticField::new("poll_ms", format!("{} ms", self.poll_ms)),
            DiagnosticField::new("calibration_preset", self.calibration_preset.clone()),
            DiagnosticField::new(
                "calibration_output_gain",
                format!("{:.2}x", self.calibration_output_gain),
            ),
            DiagnosticField::new(
                "calibration_const_gain",
                format!("{:.2}x", self.calibration_const_gain),
            ),
            DiagnosticField::new(
                "calibration_sine_gain",
                format!("{:.2}x", self.calibration_sine_gain),
            ),
            DiagnosticField::new(
                "calibration_spring_gain",
                format!("{:.2}x", self.calibration_spring_gain),
            ),
            DiagnosticField::new(
                "calibration_damper_gain",
                format!("{:.2}x", self.calibration_damper_gain),
            ),
            DiagnosticField::new(
                "calibration_center_offset",
                format!("{:+.0}%", self.calibration_center_offset * 100.0),
            ),
            DiagnosticField::new(
                "calibration_steering_range",
                format!("{:.2}x", self.calibration_range),
            ),
            DiagnosticField::new(
                "calibration_steering_curve",
                format!("{:.2}", self.calibration_curve),
            ),
            DiagnosticField::new(
                "filter_coefficient",
                self.steering_filter_coefficient
                    .map(|value| format!("{:.0}%", value * 100.0))
                    .unwrap_or_else(|| "Unavailable".to_string()),
            ),
            DiagnosticField::new(
                "filter_coefficient_value",
                self.steering_filter_coefficient
                    .map(|value| format!("{value:.4}"))
                    .unwrap_or_else(|| "0.0000".to_string()),
            ),
            DiagnosticField::new(
                "hot_reload",
                if self.hot_reload_enabled {
                    "On save"
                } else {
                    "Manual"
                },
            ),
            DiagnosticField::new("updates_total", self.packet_updates_total.to_string()),
            DiagnosticField::new("commands_total", self.applied_commands_total.to_string()),
        ]
    }

    fn snapshot_if_due(&mut self) -> Option<Vec<DiagnosticField>> {
        let now = Instant::now();
        if now.saturating_duration_since(self.last_snapshot_at) < TELEMETRY_INTERVAL {
            return None;
        }

        let fields = self.snapshot_fields();
        self.last_snapshot_at = now;
        self.packet_updates_since_snapshot = 0;
        self.applied_commands_since_snapshot = 0;
        Some(fields)
    }
}

fn format_elapsed(duration: Duration) -> String {
    if duration.as_millis() < 1_000 {
        return format!("{} ms", duration.as_millis());
    }

    let seconds = duration.as_secs_f32();
    if seconds < 60.0 {
        return format!("{seconds:.1} s");
    }

    let minutes = duration.as_secs() / 60;
    let remaining_seconds = duration.as_secs() % 60;
    format!("{}m {}s", minutes, remaining_seconds)
}

fn format_steering_state(steering_state: Option<i32>) -> String {
    let Some(raw_value) = steering_state else {
        return "Unavailable".to_string();
    };

    let centered = raw_value - 32_768;
    let percent = (centered as f32 / 32_767.0) * 100.0;
    format!("{percent:+.0}% ({raw_value})")
}

fn steering_input_label(input: Option<&WinmmJoystick>) -> String {
    input
        .map(|input| format!("{} [WinMM #{}]", input.name(), input.id()))
        .unwrap_or_else(|| "No steering input".to_string())
}

struct CommandForceTelemetry {
    peak_force_ratio: f32,
    condition_saturation_ratio: f32,
    peak_force_label: String,
    clamp_status: String,
    saturation_status: String,
}

fn analyze_wheel_commands(commands: &[WheelCommand]) -> CommandForceTelemetry {
    let mut peak_force_ratio = 0.0f32;
    let mut peak_source = "No active force output".to_string();
    let mut condition_saturation_ratio = 0.0f32;
    let mut saturation_source: Option<String> = None;

    for command in commands {
        match command {
            WheelCommand::Constant(command) => update_peak_force(
                &mut peak_force_ratio,
                &mut peak_source,
                "Constant".to_string(),
                command.magnitude.abs() as f32 / 10_000.0,
            ),
            WheelCommand::Periodic(command) => update_peak_force(
                &mut peak_force_ratio,
                &mut peak_source,
                "Periodic".to_string(),
                command.magnitude.abs() as f32 / 10_000.0,
            ),
            WheelCommand::Condition(command) => {
                update_peak_force(
                    &mut peak_force_ratio,
                    &mut peak_source,
                    format!("{:?} coefficient", command.kind),
                    command
                        .positive_coefficient
                        .abs()
                        .max(command.negative_coefficient.abs()) as f32
                        / 10_000.0,
                );

                let saturation_ratio =
                    command.positive_saturation.max(command.negative_saturation) as f32 / 10_000.0;
                if saturation_ratio > condition_saturation_ratio {
                    condition_saturation_ratio = saturation_ratio;
                    saturation_source = Some(format!("{:?}", command.kind));
                }
            }
            WheelCommand::Ignore | WheelCommand::DeviceControl(_) | WheelCommand::StopEffect(_) => {
            }
        }
    }

    let peak_force_label = format!("{:.0}%", (peak_force_ratio * 100.0).clamp(0.0, 100.0));
    let clamp_status = if peak_force_ratio == 0.0 {
        "No active force output.".to_string()
    } else if peak_force_ratio >= 0.98 {
        format!(
            "{peak_source} reached clamp at {:.0}%.",
            peak_force_ratio * 100.0
        )
    } else if peak_force_ratio >= 0.85 {
        format!(
            "{peak_source} is near clamp at {:.0}%.",
            peak_force_ratio * 100.0
        )
    } else {
        format!(
            "{peak_source} has headroom at {:.0}% peak.",
            peak_force_ratio * 100.0
        )
    };
    let saturation_status = match saturation_source {
        Some(source) if condition_saturation_ratio >= 0.98 => {
            format!(
                "{source} saturation is capped at {:.0}%.",
                condition_saturation_ratio * 100.0
            )
        }
        Some(source) => {
            format!(
                "{source} saturation is {:.0}%.",
                condition_saturation_ratio * 100.0
            )
        }
        None => "No condition saturation activity.".to_string(),
    };

    CommandForceTelemetry {
        peak_force_ratio,
        condition_saturation_ratio,
        peak_force_label,
        clamp_status,
        saturation_status,
    }
}

fn update_peak_force(
    peak_force_ratio: &mut f32,
    peak_source: &mut String,
    source: String,
    ratio: f32,
) {
    if ratio > *peak_force_ratio {
        *peak_force_ratio = ratio.clamp(0.0, 1.0);
        *peak_source = source;
    }
}

fn summarize_effect_updates(updates: &[EffectUpdate]) -> String {
    summarize_items(updates, describe_effect_update)
}

fn summarize_wheel_commands(commands: &[WheelCommand]) -> String {
    summarize_items(commands, describe_wheel_command)
}

fn summarize_items<T>(items: &[T], describe: impl Fn(&T) -> String) -> String {
    if items.is_empty() {
        return "none".to_string();
    }

    const MAX_ITEMS: usize = 4;
    let mut summary = items
        .iter()
        .take(MAX_ITEMS)
        .map(describe)
        .collect::<Vec<_>>()
        .join("; ");
    if items.len() > MAX_ITEMS {
        summary.push_str(&format!("; +{} more", items.len() - MAX_ITEMS));
    }
    summary
}

fn describe_effect_update(update: &EffectUpdate) -> String {
    match update {
        EffectUpdate::Ignore => "ignore".to_string(),
        EffectUpdate::DeviceControl(control) => format!("device_control({control:?})"),
        EffectUpdate::StopEffect(kind) => format!("stop({kind:?})"),
        EffectUpdate::Apply(effect) => describe_game_effect(effect),
    }
}

fn describe_game_effect(effect: &GameEffect) -> String {
    match effect {
        GameEffect::Constant {
            metadata,
            magnitude,
        } => format!(
            "constant(magnitude={magnitude}, gain={}, duration_ms={})",
            metadata.gain, metadata.duration_ms,
        ),
        GameEffect::Periodic {
            metadata,
            magnitude,
            offset,
            period,
            phase,
        } => format!(
            "periodic(magnitude={magnitude}, offset={offset}, period={period}, phase={phase}, gain={})",
            metadata.gain,
        ),
        GameEffect::Condition {
            metadata,
            kind,
            positive_coefficient,
            negative_coefficient,
            positive_saturation,
            negative_saturation,
            ..
        } => format!(
            "condition(kind={kind:?}, pos_coef={positive_coefficient}, neg_coef={negative_coefficient}, pos_sat={positive_saturation}, neg_sat={negative_saturation}, gain={})",
            metadata.gain,
        ),
    }
}

fn describe_wheel_command(command: &WheelCommand) -> String {
    match command {
        WheelCommand::Ignore => "ignore".to_string(),
        WheelCommand::DeviceControl(control) => format!("device_control({control:?})"),
        WheelCommand::StopEffect(kind) => format!("stop({kind:?})"),
        WheelCommand::Constant(command) => format!(
            "constant(magnitude={}, gain={}, direction={})",
            command.magnitude, command.metadata.gain, command.metadata.direction,
        ),
        WheelCommand::Periodic(PeriodicCommand {
            metadata,
            magnitude,
            offset,
            period,
            phase,
        }) => format!(
            "periodic(magnitude={magnitude}, offset={offset}, period={period}, phase={phase}, gain={})",
            metadata.gain,
        ),
        WheelCommand::Condition(ConditionCommand {
            metadata,
            kind,
            positive_coefficient,
            negative_coefficient,
            positive_saturation,
            negative_saturation,
            ..
        }) => format!(
            "condition(kind={kind:?}, pos_coef={positive_coefficient}, neg_coef={negative_coefficient}, pos_sat={positive_saturation}, neg_sat={negative_saturation}, gain={})",
            metadata.gain,
        ),
    }
}
