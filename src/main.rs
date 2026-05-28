use anyhow::{Result, anyhow};
use clap::{Parser, Subcommand};
use forzabeast::backends::directinput::DirectInput;
use forzabeast::backends::ffbeast::{DirectControl, FFBeastBackend};
use forzabeast::config::{ControllerConfig, FfbParamsConfig, load_controllers};
use forzabeast::core::domain::{EffectMetadata, EffectUpdate, GameEffect};
use forzabeast::effect_engine::EffectEngine;
use forzabeast::frontends::forza_vjoy::{
    InputFrame, InputMapper, RegisteredFfbCallback, VJoyDevice,
};
use forzabeast::inputs::WinmmJoystick;
use forzabeast::profile::{FfbProfile, ProfileWatcher, load_profile};
use forzabeast::ui::run_profile_editor;
use std::thread;
use std::time::Duration;

#[derive(Debug, Parser)]
#[command(
    name = "forzabeast",
    about = "Native FFBeast HID probe for replacing vJoy/EmuWheel"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
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
        #[arg(long, default_value = "C:/Users/justi/Torquebridge/configuration.json")]
        config: String,
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
        #[arg(long, default_value_t = 1)]
        id: u32,
        #[arg(long)]
        steering_device: Option<u32>,
        #[arg(long)]
        poll_ms: Option<u64>,
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

    match cli.cmd {
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
            run_profile_editor(&config, profile.as_deref())?;
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
            id,
            steering_device,
            poll_ms,
        } => {
            let (controllers, effective_profile) =
                load_effective_profile(&config, profile.as_deref())?;
            let settings = effective_profile.ffb_parameters.clone();
            let cli_steering_device = steering_device;
            let cli_poll_ms = poll_ms;
            let mut steering_device =
                effective_profile.resolved_steering_device(cli_steering_device);
            let mut poll_ms = effective_profile.resolved_poll_ms(cli_poll_ms, 5);
            let mut profile_watcher = profile.as_deref().map(ProfileWatcher::new).transpose()?;

            let vjoy = VJoyDevice::initialize(id)?;
            if !vjoy.is_ffb_capable() {
                return Err(anyhow!("vJoy device {id} is not FFB-capable"));
            }

            let callback = RegisteredFfbCallback::register(vjoy, settings)?;
            let direct_input = DirectInput::create()?;
            let mut output = direct_input.open_configured_ffb_device(&controllers)?;
            let mut steering_input = steering_device.map(WinmmJoystick::open).transpose()?;

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

            if let Some(watcher) = profile_watcher.as_ref() {
                println!(
                    "watching profile file {} for changes",
                    watcher.path().display()
                );
            }

            print_steering_input_status(steering_input.as_ref());

            loop {
                if let Some(watcher) = profile_watcher.as_mut() {
                    match watcher.reload_if_changed() {
                        Ok(Some(reloaded_profile)) => {
                            callback.replace_settings(reloaded_profile.ffb_parameters.clone())?;

                            let next_poll_ms = reloaded_profile.resolved_poll_ms(cli_poll_ms, 5);
                            if next_poll_ms != poll_ms {
                                poll_ms = next_poll_ms;
                                println!("updated profile poll interval to {} ms", poll_ms);
                            }

                            let next_steering_device =
                                reloaded_profile.resolved_steering_device(cli_steering_device);
                            if next_steering_device != steering_device {
                                steering_input =
                                    next_steering_device.map(WinmmJoystick::open).transpose()?;
                                steering_device = next_steering_device;
                                print_steering_input_status(steering_input.as_ref());
                            }

                            println!(
                                "reloaded force profile {}",
                                reloaded_profile.name.as_deref().unwrap_or("<unnamed>")
                            );
                        }
                        Ok(None) => {}
                        Err(error) => {
                            eprintln!(
                                "profile reload failed for {}: {}",
                                watcher.path().display(),
                                error
                            );
                        }
                    }
                }

                if let Some(input) = steering_input.as_mut() {
                    let frame = input.poll_input_frame()?;
                    callback.update_steering_state(frame.x)?;
                }

                if let Some(update) = callback.take_pending_update()? {
                    output.apply_commands(&update.commands)?;
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

fn load_effective_profile(
    config: &str,
    profile_path: Option<&str>,
) -> Result<(Vec<ControllerConfig>, FfbProfile)> {
    let controllers = load_controllers(config)?;
    let profile = if let Some(path) = profile_path {
        load_profile(path)?
    } else {
        FfbProfile::from_ffb_settings(load_ffb_settings(&controllers)?)
    };

    Ok((controllers, profile))
}

fn load_ffb_settings(controllers: &[ControllerConfig]) -> Result<FfbParamsConfig> {
    controllers
        .iter()
        .find_map(|controller| controller.ffb_parameters.clone())
        .ok_or_else(|| anyhow!("no FFBParameters entry found in config"))
}

fn print_steering_input_status(input: Option<&WinmmJoystick>) {
    if let Some(input) = input {
        println!(
            "using WinMM input device {} ({}) for live steering-state updates",
            input.id(),
            input.name()
        );
    } else {
        println!(
            "no live steering input selected; run 'forzabeast list-inputs' and pass --steering-device <id> to enable steering-state updates"
        );
    }
}
