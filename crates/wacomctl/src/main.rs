use std::io::{self, BufReader, Read};
use std::os::unix::net::UnixStream;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use wacom_proto::config::{self, Action, Config, Profile, ScreenSelection};
use wacom_proto::ipc::{
    read_message, socket_path, write_message, Event, Request, Status, Telemetry,
};
use wacom_proto::transform::{NormRect, Orientation};

#[derive(Parser)]
#[command(
    name = "wacomctl",
    version,
    about = "Control the Wacom Intuos S (CTL-4100)"
)]
struct Cli {
    /// Machine-readable JSON output.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show device status, profile and live telemetry.
    Status,
    /// Stream live pen/pad telemetry.
    Listen,
    /// Inspect or replace the whole configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Change one setting.
    Set {
        #[command(subcommand)]
        command: SetCommand,
    },
    /// Manage per-application profiles stored in the config.
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    /// Pin the effective profile (or follow Hyprland focus again).
    Use {
        app: Option<String>,
        /// Clear the pin and follow the focused window.
        #[arg(long)]
        follow: bool,
    },
    /// Show the radial menu in wacom-gui.
    Radial,
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Print the effective configuration.
    Get,
    /// Print the configuration file path.
    Path,
    /// Replace the configuration from a JSON file ("-" = stdin).
    Set {
        #[arg(default_value = "-")]
        file: String,
    },
}

#[derive(Subcommand)]
enum SetCommand {
    /// Tip feel, 0 (soft) .. 100 (firm).
    Pressure { value: f32 },
    /// Tablet orientation in degrees (0, 90, 180, 270).
    Orientation { degrees: u16 },
    /// Active tablet area, normalized 0..1 (x y w h).
    Area { x: f64, y: f64, w: f64, h: f64 },
    /// Keep the tablet aspect ratio on screen.
    Proportions { value: OnOff },
    /// Map to all monitors or to one Hyprland monitor.
    Monitor { name: String },
    /// Double-click assist distance (px) and time (ms).
    DoubleClick { distance: f32, time_ms: u64 },
    /// Remap a button to an action (JSON, see docs).
    Action {
        target: ButtonTarget,
        #[arg(value_name = "ACTION_JSON")]
        spec: String,
    },
    /// Remove a custom pressure curve (back to the Tip Feel slider).
    LinearCurve,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum OnOff {
    On,
    Off,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum ButtonTarget {
    Pen1,
    Pen2,
    Key1,
    Key2,
    Key3,
    Key4,
}

#[derive(Subcommand)]
enum ProfileCommand {
    /// List configured profile keys.
    List,
    /// Print the configuration an app would get.
    Show { app: String },
    /// Create an empty profile for an app class.
    Add { app: String },
    /// Remove a profile.
    Remove { app: String },
}

struct Client {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
}

impl Client {
    fn connect() -> Result<Self> {
        let path = socket_path();
        let stream = UnixStream::connect(&path).with_context(|| {
            format!("cannot connect to {} (is wacomd running?)", path.display())
        })?;
        let writer = stream.try_clone()?;
        let mut reader = BufReader::new(stream);
        let hello =
            read_message::<_, Event>(&mut reader)?.context("daemon closed the connection")?;
        match hello {
            Event::Hello { version } if version == wacom_proto::ipc::PROTOCOL_VERSION => {}
            Event::Hello { version } => bail!("protocol mismatch: daemon speaks v{version}"),
            _ => bail!("unexpected greeting from daemon"),
        }
        Ok(Self { writer, reader })
    }

    fn send(&mut self, request: &Request) -> Result<()> {
        write_message(&mut self.writer, request)?;
        Ok(())
    }

    fn next(&mut self) -> Result<Event> {
        read_message::<_, Event>(&mut self.reader)?.context("daemon closed the connection")
    }

    fn request(&mut self, request: Request) -> Result<Event> {
        self.send(&request)?;
        loop {
            match self.next()? {
                Event::Telemetry(_) | Event::Pad { .. } | Event::Radial => continue,
                Event::Error { message } => bail!("{message}"),
                event => return Ok(event),
            }
        }
    }

    fn config(&mut self) -> Result<Config> {
        match self.request(Request::GetConfig)? {
            Event::Config(cfg) => Ok(cfg),
            _ => bail!("unexpected response"),
        }
    }

    fn set_config(&mut self, cfg: Config) -> Result<Config> {
        match self.request(Request::SetConfig(cfg))? {
            Event::Config(cfg) => Ok(cfg),
            _ => bail!("unexpected response"),
        }
    }
}

fn main() {
    // Match normal Unix CLI behaviour when piped into `head`.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    if let Err(e) = run() {
        eprintln!("wacomctl: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Status => {
            let mut c = Client::connect()?;
            match c.request(Request::Status)? {
                Event::Status(status) => print_status(&status, cli.json),
                _ => bail!("unexpected response"),
            }
        }
        Command::Listen => listen(cli.json)?,
        Command::Config { command } => match command {
            ConfigCommand::Get => {
                let mut c = Client::connect()?;
                let cfg = c.config()?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&cfg)?);
                } else {
                    print_config(&cfg);
                }
            }
            ConfigCommand::Path => println!("{}", config_path().display()),
            ConfigCommand::Set { file } => {
                let text = if file == "-" {
                    let mut s = String::new();
                    io::stdin().read_to_string(&mut s)?;
                    s
                } else {
                    std::fs::read_to_string(&file)?
                };
                let cfg: Config = serde_json::from_str(&text)?;
                let mut c = Client::connect()?;
                let cfg = c.set_config(cfg)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&cfg)?);
                } else {
                    println!(
                        "configuration saved to {}",
                        config::default_path().display()
                    );
                }
            }
        },
        Command::Set { command } => set(cli.json, command)?,
        Command::Profile { command } => profile(cli.json, command)?,
        Command::Use { app, follow } => {
            let pin = if follow { None } else { app };
            let mut c = Client::connect()?;
            c.request(Request::PinProfile(pin.clone()))?;
            let mut c = Client::connect()?;
            if let Event::Status(status) = c.request(Request::Status)? {
                if cli.json {
                    println!("{}", serde_json::to_string(&status)?);
                } else if let Some(app) = pin {
                    println!("pinned profile: {app}");
                } else {
                    println!("following focused window");
                }
            }
        }
        Command::Radial => {
            let mut c = Client::connect()?;
            c.request(Request::RunAction(Action::RadialMenu))?;
        }
    }
    Ok(())
}

fn set(json: bool, command: SetCommand) -> Result<()> {
    let mut c = Client::connect()?;
    let mut cfg = c.config()?;
    match command {
        SetCommand::Pressure { value } => {
            if !(0.0..=100.0).contains(&value) {
                bail!("pressure must be within 0..=100");
            }
            cfg.pen.tip_feel = value / 100.0;
            cfg.pen.custom_curve = None;
        }
        SetCommand::Orientation { degrees } => {
            cfg.mapping.orientation = Orientation::from_degrees(degrees)
                .context("orientation must be 0, 90, 180 or 270")?;
        }
        SetCommand::Area { x, y, w, h } => {
            if w <= 0.0 || h <= 0.0 {
                bail!("width and height must be positive");
            }
            cfg.mapping.tablet_area = NormRect::new(x, y, w, h).sanitized();
        }
        SetCommand::Proportions { value } => {
            cfg.mapping.force_proportions = matches!(value, OnOff::On);
        }
        SetCommand::Monitor { name } => {
            cfg.mapping.screen = if name.eq_ignore_ascii_case("all") {
                ScreenSelection::All
            } else {
                ScreenSelection::Monitor(name)
            };
        }
        SetCommand::DoubleClick { distance, time_ms } => {
            cfg.pen.double_click_distance = distance.max(0.0);
            cfg.pen.double_click_time_ms = time_ms;
        }
        SetCommand::Action { target, spec } => {
            let action: Action = serde_json::from_str(&spec)
                .context("action must be JSON, e.g. '{\"type\":\"key\",\"value\":{\"mods\":[\"ctrl\"],\"key\":\"z\"}}'")?;
            match target {
                ButtonTarget::Pen1 => cfg.pen.button1 = action,
                ButtonTarget::Pen2 => cfg.pen.button2 = action,
                ButtonTarget::Key1 => cfg.expresskeys[0] = action,
                ButtonTarget::Key2 => cfg.expresskeys[1] = action,
                ButtonTarget::Key3 => cfg.expresskeys[2] = action,
                ButtonTarget::Key4 => cfg.expresskeys[3] = action,
            }
        }
        SetCommand::LinearCurve => {
            cfg.pen.custom_curve = None;
        }
    }
    let cfg = c.set_config(cfg)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&cfg)?);
    } else {
        println!("updated");
    }
    Ok(())
}

fn profile(json: bool, command: ProfileCommand) -> Result<()> {
    match command {
        ProfileCommand::List => {
            let cfg = match Client::connect() {
                Ok(mut c) => c.config()?,
                Err(_) => config::load(&config_path())?,
            };
            if json {
                let keys: Vec<&String> = cfg.profiles.keys().collect();
                println!("{}", serde_json::to_string(&keys)?);
            } else if cfg.profiles.is_empty() {
                println!("no profiles");
            } else {
                for key in cfg.profiles.keys() {
                    println!("{key}");
                }
            }
        }
        ProfileCommand::Show { app } => {
            let cfg = match Client::connect() {
                Ok(mut c) => c.config()?,
                Err(_) => config::load(&config_path())?,
            };
            let effective = cfg.effective(Some(&app));
            if json {
                println!("{}", serde_json::to_string_pretty(&effective)?);
            } else {
                println!("profile: {app}");
                println!("  tip feel: {:.0}%", effective.pen.tip_feel * 100.0);
                println!(
                    "  orientation: {}°",
                    effective.mapping.orientation.degrees()
                );
                println!(
                    "  tablet area: x={:.3} y={:.3} w={:.3} h={:.3}",
                    effective.mapping.tablet_area.x,
                    effective.mapping.tablet_area.y,
                    effective.mapping.tablet_area.w,
                    effective.mapping.tablet_area.h
                );
                for (i, action) in effective.expresskeys.iter().enumerate() {
                    println!("  key {}: {}", i + 1, action.label());
                }
            }
        }
        ProfileCommand::Add { app } => {
            let mut c = Client::connect()?;
            let mut cfg = c.config()?;
            cfg.profiles.insert(
                app.clone(),
                Profile {
                    pen: Some(cfg.pen.clone()),
                    expresskeys: Some(cfg.expresskeys.clone()),
                    mapping: Some(cfg.mapping.clone()),
                },
            );
            c.set_config(cfg)?;
            println!("profile added: {app}");
        }
        ProfileCommand::Remove { app } => {
            let mut c = Client::connect()?;
            let mut cfg = c.config()?;
            if cfg.profiles.remove(&app).is_none() {
                bail!("no profile named {app}");
            }
            c.set_config(cfg)?;
            println!("profile removed: {app}");
        }
    }
    Ok(())
}

fn listen(json: bool) -> Result<()> {
    let mut c = Client::connect()?;
    c.send(&Request::Subscribe)?;
    loop {
        let event = c.next()?;
        match event {
            Event::Telemetry(t) => {
                if json {
                    println!("{}", serde_json::to_string(&t)?);
                } else {
                    println!("{}", format_telemetry(&t));
                }
            }
            Event::Pad { index, pressed } => {
                if json {
                    println!("{}", serde_json::to_string(&Event::Pad { index, pressed })?);
                } else {
                    println!("key {} {}", index + 1, if pressed { "down" } else { "up" });
                }
            }
            Event::Radial => {
                if !json {
                    println!("radial menu");
                }
            }
            Event::Error { message } => bail!("{message}"),
            _ => {}
        }
    }
}

fn format_telemetry(t: &Telemetry) -> String {
    format!(
        "x={:5} y={:5} p={:4}/{:4} raw={:4} in={} tip={} b1={} b2={} er={} keys={:04b} pps={:.0}",
        t.x,
        t.y,
        t.pressure,
        wacom_proto::PRESSURE_MAX,
        t.raw_pressure,
        u8::from(t.in_range),
        u8::from(t.tip),
        u8::from(t.barrel),
        u8::from(t.barrel2),
        u8::from(t.eraser),
        t.keys,
        t.pps
    )
}

fn config_path() -> std::path::PathBuf {
    std::env::var_os("WACOMD_CONFIG")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(config::default_path)
}

fn print_status(s: &Status, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(s).unwrap_or_default());
        return;
    }
    println!("{}", s.product);
    println!(
        "  device:    {}",
        if s.connected {
            "connected"
        } else {
            "not connected"
        }
    );
    if let Some(serial) = &s.serial {
        println!("  serial:    {serial}");
    }
    println!("  mode:      {:?}", s.mode);
    println!(
        "  profile:   {}{}",
        s.active_profile.as_deref().unwrap_or("global"),
        if s.pinned { " (pinned)" } else { "" }
    );
    if let Some(app) = &s.app_class {
        println!("  app:       {app}");
    }
    if s.monitors.is_empty() {
        println!("  monitors:  none");
    } else {
        for m in &s.monitors {
            println!(
                "  monitor:   {} {:.0}x{:.0} at {:.0},{:.0}{}",
                m.name,
                m.width,
                m.height,
                m.x,
                m.y,
                if m.focused { " (focused)" } else { "" }
            );
        }
    }
    let layout = s.layout;
    println!(
        "  layout:    {:.0}x{:.0} at {:.0},{:.0}",
        layout.w, layout.h, layout.x, layout.y
    );
    if let Some(t) = &s.telemetry {
        println!("  telemetry: {}", format_telemetry(t));
    }
    println!("  config:    {}", s.config_path);
}

fn print_config(cfg: &Config) {
    println!("tip feel:      {:.0}%", cfg.pen.tip_feel * 100.0);
    println!("orientation:   {}°", cfg.mapping.orientation.degrees());
    println!(
        "tablet area:   x={:.3} y={:.3} w={:.3} h={:.3}",
        cfg.mapping.tablet_area.x,
        cfg.mapping.tablet_area.y,
        cfg.mapping.tablet_area.w,
        cfg.mapping.tablet_area.h
    );
    println!(
        "screen:        {}",
        match &cfg.mapping.screen {
            ScreenSelection::All => "all monitors".to_string(),
            ScreenSelection::Monitor(name) => name.clone(),
        }
    );
    println!("proportions:   {}", cfg.mapping.force_proportions);
    println!("pen button 1:  {}", cfg.pen.button1.label());
    println!("pen button 2:  {}", cfg.pen.button2.label());
    for (i, action) in cfg.expresskeys.iter().enumerate() {
        println!("express key {}: {}", i + 1, action.label());
    }
    if !cfg.profiles.is_empty() {
        println!(
            "profiles:      {}",
            cfg.profiles.keys().cloned().collect::<Vec<_>>().join(", ")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_line_is_stable() {
        let t = Telemetry {
            x: 15200,
            y: 9500,
            pressure: 2048,
            raw_pressure: 4095,
            tip: true,
            in_range: true,
            keys: 0b0101,
            pps: 133.4,
            ..Telemetry::default()
        };
        let line = format_telemetry(&t);
        assert!(line.contains("x=15200"));
        assert!(line.contains("p=2048/4095"));
        assert!(line.contains("keys=0101"));
        assert!(line.contains("pps=133"));
    }
}
