mod daemon;
mod ipc;
mod layout;
mod position;
mod wayland;

use ipc::{Direction, SnapCommand};

fn main() {
    env_logger::init();

    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() || args[0] == "daemon" {
        // No args or "daemon" → run as daemon
        daemon::run_daemon();
        return;
    }

    // Otherwise, parse as a CLI command and send to the daemon
    let cmd = match args[0].as_str() {
        "tile"          => SnapCommand::Tile,
        "float"         => SnapCommand::Float,
        "tile-all"      => SnapCommand::TileAll,
        "swap-master"   => SnapCommand::SwapMaster,
        "grow-master"   => SnapCommand::GrowMaster,
        "shrink-master" => SnapCommand::ShrinkMaster,
        "reset"         => SnapCommand::Reset,
        "quit"          => SnapCommand::Quit,
        "focus" => {
            let dir = args.get(1).and_then(|d| match d.as_str() {
                "left"  => Some(Direction::Left),
                "right" => Some(Direction::Right),
                "up"    => Some(Direction::Up),
                "down"  => Some(Direction::Down),
                _ => None,
            });
            match dir {
                Some(d) => SnapCommand::Focus(d),
                None => {
                    eprintln!("Usage: rdm-snap focus <left|right|up|down>");
                    std::process::exit(1);
                }
            }
        }
        "help" | "--help" | "-h" => {
            print_help();
            return;
        }
        other => {
            eprintln!("Unknown command: {}", other);
            print_help();
            std::process::exit(1);
        }
    };

    match ipc::send_command(&cmd) {
        Ok(resp) => {
            if resp.starts_with("error") {
                eprintln!("{}", resp);
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("rdm-snap: {}", e);
            eprintln!("Is the rdm-snap daemon running? Start it with: rdm-snap daemon");
            std::process::exit(1);
        }
    }
}

fn print_help() {
    println!(
        "rdm-snap — tiling window manager for RDM/labwc

Usage:
  rdm-snap                  Start the daemon
  rdm-snap daemon           Start the daemon (explicit)
  rdm-snap tile             Tile the focused window
  rdm-snap float            Float the focused window (remove from tiling)
  rdm-snap tile-all         Tile all open windows
  rdm-snap focus <dir>      Focus a tiled window (left/right/up/down)
  rdm-snap swap-master      Swap focused window with master
  rdm-snap grow-master      Increase master pane width
  rdm-snap shrink-master    Decrease master pane width
  rdm-snap reset            Rebuild layout (remove dead windows)
  rdm-snap quit             Stop the daemon"
    );
}
