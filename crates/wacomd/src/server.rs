//! Unix-socket IPC server shared by the daemon, CLI and GUI.

use std::fs;
use std::io::{self, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use wacom_proto::config::{self, Action, Config};
use wacom_proto::ipc::{
    read_message, socket_path, write_message, Event, Request, Status, PROTOCOL_VERSION,
};

#[allow(clippy::large_enum_variant)] // Config is rare; boxing it would only add churn.
#[derive(Debug)]
pub enum Command {
    Config(Config),
    Action(Action),
    Pin(Option<String>),
}

pub struct Shared {
    pub config: Mutex<Config>,
    pub status: Mutex<Status>,
    subscribers: Mutex<Vec<UnixStream>>,
    config_path: std::path::PathBuf,
}

impl Shared {
    pub fn new(config: Config, status: Status, config_path: std::path::PathBuf) -> Arc<Self> {
        Arc::new(Self {
            config: Mutex::new(config),
            status: Mutex::new(status),
            subscribers: Mutex::new(Vec::new()),
            config_path,
        })
    }

    pub fn has_subscribers(&self) -> bool {
        self.subscribers
            .lock()
            .map(|s| !s.is_empty())
            .unwrap_or(false)
    }

    pub fn broadcast(&self, event: &Event) {
        let Ok(mut subs) = self.subscribers.lock() else {
            return;
        };
        subs.retain_mut(|stream| write_message(stream, event).is_ok());
    }

    fn subscribe(&self, stream: UnixStream) {
        let _ = stream.set_write_timeout(Some(Duration::from_millis(250)));
        if let Ok(mut subs) = self.subscribers.lock() {
            subs.push(stream);
        }
    }
}

pub fn spawn(shared: Arc<Shared>, tx: Sender<Command>) -> io::Result<()> {
    let path = socket_path();
    if path.exists() {
        if UnixStream::connect(&path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "another wacomd is already listening",
            ));
        }
        fs::remove_file(&path)?;
    }
    let listener = UnixListener::bind(&path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let shared = Arc::clone(&shared);
            let tx = tx.clone();
            thread::spawn(move || handle(stream, shared, tx));
        }
    });
    Ok(())
}

fn handle(stream: UnixStream, shared: Arc<Shared>, tx: Sender<Command>) {
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    if write_message(
        &mut writer,
        &Event::Hello {
            version: PROTOCOL_VERSION,
        },
    )
    .is_err()
    {
        return;
    }
    let mut reader = BufReader::new(stream);
    loop {
        let request = match read_message::<_, Request>(&mut reader) {
            Ok(Some(r)) => r,
            Ok(None) | Err(_) => return,
        };
        match request {
            Request::Ping => {
                let _ = write_message(&mut writer, &Event::Pong);
            }
            Request::Status => {
                let status = shared.status.lock().map(|s| s.clone()).unwrap_or_else(|_| {
                    Status::disconnected(shared.config_path.display().to_string())
                });
                let _ = write_message(&mut writer, &Event::Status(status));
            }
            Request::GetConfig => {
                let cfg = shared.config.lock().map(|c| c.clone()).ok();
                if let Some(cfg) = cfg {
                    let _ = write_message(&mut writer, &Event::Config(cfg));
                }
            }
            Request::SetConfig(cfg) => {
                if let Err(e) = config::save(&shared.config_path, &cfg) {
                    let _ = write_message(
                        &mut writer,
                        &Event::Error {
                            message: format!("cannot save config: {e}"),
                        },
                    );
                    continue;
                }
                if let Ok(mut guard) = shared.config.lock() {
                    *guard = cfg.clone();
                }
                let _ = tx.send(Command::Config(cfg.clone()));
                let _ = write_message(&mut writer, &Event::Config(cfg.clone()));
                shared.broadcast(&Event::Config(cfg));
            }
            Request::Subscribe => {
                shared.subscribe(writer);
                return;
            }
            Request::RunAction(action) => {
                let _ = tx.send(Command::Action(action));
                let _ = write_message(&mut writer, &Event::Ack);
            }
            Request::PinProfile(app) => {
                let _ = tx.send(Command::Pin(app));
                let _ = write_message(&mut writer, &Event::Ack);
            }
        }
    }
}
