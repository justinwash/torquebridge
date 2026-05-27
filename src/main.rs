mod constants;
mod device;
mod protocol;

use anyhow::Result;
use clap::{Parser, Subcommand};
use device::FFBeastDevice;
use protocol::DirectControl;
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
            let _dev = FFBeastDevice::connect()?;
            println!("Connected to FFBeast (045B:59D7)");
        }
        Command::State { count, timeout_ms } => {
            let dev = FFBeastDevice::connect()?;
            for _ in 0..count {
                let state = dev.read_state_blocking(timeout_ms)?;
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
            let dev = FFBeastDevice::connect()?;
            let cmd = DirectControl {
                spring_force: spring,
                constant_force: constant,
                periodic_force: periodic,
                force_drop: drop,
            };

            dev.send_direct_control(cmd)?;
            println!("Applied direct control: {:?}", cmd.clamped());

            if hold_ms > 0 {
                thread::sleep(Duration::from_millis(hold_ms));
                dev.send_direct_control(DirectControl {
                    spring_force: 0,
                    constant_force: 0,
                    periodic_force: 0,
                    force_drop: 0,
                })?;
                println!("Returned force outputs to zero");
            }
        }
        Command::Gain { percent } => {
            let dev = FFBeastDevice::connect()?;
            dev.set_device_gain(percent)?;
            println!("Set device gain to {}%", percent.min(100));
        }
    }

    Ok(())
}
