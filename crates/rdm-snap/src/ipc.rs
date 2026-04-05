use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

/// Socket path for the rdm-snap daemon.
pub fn socket_path() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR")
        .map(|d| PathBuf::from(d).join("rdm-snap.sock"))
        .unwrap_or_else(|_| PathBuf::from("/tmp/rdm-snap.sock"))
}

/// Commands the CLI can send to the daemon.
#[derive(Debug, Clone)]
pub enum SnapCommand {
    Tile,
    Float,
    TileAll,
    Focus(Direction),
    SwapMaster,
    GrowMaster,
    ShrinkMaster,
    Reset,
    Quit,
}

#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl SnapCommand {
    /// Serialize to a simple line protocol.
    pub fn to_wire(&self) -> String {
        match self {
            Self::Tile => "tile".into(),
            Self::Float => "float".into(),
            Self::TileAll => "tile-all".into(),
            Self::Focus(d) => format!("focus {}", d.as_str()),
            Self::SwapMaster => "swap-master".into(),
            Self::GrowMaster => "grow-master".into(),
            Self::ShrinkMaster => "shrink-master".into(),
            Self::Reset => "reset".into(),
            Self::Quit => "quit".into(),
        }
    }

    /// Deserialize from a wire line.
    pub fn from_wire(line: &str) -> Option<Self> {
        let parts: Vec<&str> = line.trim().splitn(2, ' ').collect();
        match parts[0] {
            "tile" => Some(Self::Tile),
            "float" => Some(Self::Float),
            "tile-all" => Some(Self::TileAll),
            "swap-master" => Some(Self::SwapMaster),
            "grow-master" => Some(Self::GrowMaster),
            "shrink-master" => Some(Self::ShrinkMaster),
            "reset" => Some(Self::Reset),
            "quit" => Some(Self::Quit),
            "focus" => {
                let dir = parts.get(1).and_then(|d| Direction::parse(d))?;
                Some(Self::Focus(dir))
            }
            _ => None,
        }
    }
}

impl Direction {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Up => "up",
            Self::Down => "down",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "up" => Some(Self::Up),
            "down" => Some(Self::Down),
            _ => None,
        }
    }
}

/// Send a command to the running daemon and print the response.
pub fn send_command(cmd: &SnapCommand) -> Result<String, String> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path)
        .map_err(|e| format!("Cannot connect to rdm-snap daemon at {:?}: {}", path, e))?;
    let msg = format!("{}\n", cmd.to_wire());
    stream.write_all(msg.as_bytes()).map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())?;
    // Shut down the write side so the daemon knows we're done.
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let mut reader = BufReader::new(&stream);
    let mut response = String::new();
    reader.read_line(&mut response).map_err(|e| e.to_string())?;
    Ok(response.trim().to_string())
}
