use anyhow::{Result, anyhow};
use clap::{Parser, Subcommand};
use forzabeast::backends::ffbeast::{DirectControl, FFBeastBackend};
use forzabeast::backends::ffbeast_direct::FFBeastDirectAdapter;
use forzabeast::config::load_controllers;
use forzabeast::core::domain::{EffectMetadata, EffectUpdate, GameEffect};
use forzabeast::effect_engine::EffectEngine;
use forzabeast::frontends::forza_vjoy::{
    InputFrame, InputMapper, RegisteredFfbCallback, VJoyDevice,
};
use forzabeast::inputs::WinmmJoystick;
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
    VJoyInit {
        #[arg(long, default_value_t = 1)]
        id: u32,
    },
    ShowConfig {
        #[arg(long, default_value = "C:/Users/justi/Torquebridge/configuration.json")]
        config: String,
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
    },
    FfbBridge {
        #[arg(long, default_value = "C:/Users/justi/Torquebridge/configuration.json")]
        config: String,
        #[arg(long, default_value_t = 1)]
        id: u32,
        #[arg(long)]
        steering_device: Option<u32>,
        #[arg(long, default_value_t = 5)]
        poll_ms: u64,
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
        Command::TranslateDemo { config } => {
            let controllers = load_controllers(&config)?;
            let settings = controllers
                .iter()
                .find_map(|controller| controller.ffb_parameters.clone())
                .ok_or_else(|| anyhow!("no FFBParameters entry found in config"))?;
            let mut engine = EffectEngine::new(settings);

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
            println!("translated commands: {:?}", commands);
        }
        Command::FfbBridge {
            config,
            id,
            steering_device,
            poll_ms,
        } => {
            let controllers = load_controllers(&config)?;
            let settings = controllers
                .iter()
                .find_map(|controller| controller.ffb_parameters.clone())
                .ok_or_else(|| anyhow!("no FFBParameters entry found in config"))?;

            let vjoy = VJoyDevice::initialize(id)?;
            if !vjoy.is_ffb_capable() {
                return Err(anyhow!("vJoy device {id} is not FFB-capable"));
            }

            let callback = RegisteredFfbCallback::register(vjoy, settings)?;
            let backend = FFBeastBackend::connect()?;
            let mut direct_adapter = FFBeastDirectAdapter::new();
            let mut steering_input = steering_device.map(WinmmJoystick::open).transpose()?;

            println!(
                "registered FFB bridge on vJoy device {}; forwarding translated output to FFBeast every {} ms",
                callback.id(),
                poll_ms
            );

            if let Some(input) = steering_input.as_ref() {
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

            loop {
                if let Some(input) = steering_input.as_mut() {
                    let frame = input.poll_input_frame()?;
                    callback.update_steering_state(frame.x)?;
                    if let Some(control) = direct_adapter.update_steering_state(frame.x) {
                        backend.send_direct_control(control)?;
                        println!("forwarded steering-derived direct control: {:?}", control);
                    }
                }

                if let Some(update) = callback.take_pending_update()? {
                    if let Some(control) = direct_adapter.apply_commands(&update.commands) {
                        backend.send_direct_control(control)?;
                        println!("forwarded direct control: {:?}", control);
                    }
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
