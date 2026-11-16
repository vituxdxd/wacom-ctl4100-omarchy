//! Background socket reader plus one-shot requests for the GUI.

use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use wacom_proto::ipc::{
    read_message, socket_path, write_message, Event, Request, PROTOCOL_VERSION,
};

#[allow(clippy::large_enum_variant)] // Mirrors the daemon Event enum.
pub enum UiEvent {
    Event(Event),
    Error(String),
    Disconnected(String),
}

/// Start the subscriber reader and the serialized request worker.
/// Returns (command sender, UI event receiver). The UI thread never blocks on
/// a socket request, so a slow/restarting daemon cannot freeze the window.
pub fn spawn() -> (Sender<Request>, Receiver<UiEvent>) {
    let (tx, rx) = mpsc::channel();
    let reader_tx = tx.clone();
    thread::spawn(move || reader_loop(reader_tx));
    let (cmd_tx, cmd_rx) = mpsc::channel();
    thread::spawn(move || worker_loop(cmd_rx, tx));
    (cmd_tx, rx)
}

fn reader_loop(tx: Sender<UiEvent>) {
    loop {
        match connect() {
            Ok((mut writer, mut reader)) => {
                let _ = write_message(&mut writer, &Request::Status);
                let _ = write_message(&mut writer, &Request::GetConfig);
                let _ = write_message(&mut writer, &Request::Subscribe);
                loop {
                    match read_message::<_, Event>(&mut reader) {
                        Ok(Some(event)) => {
                            if tx.send(UiEvent::Event(event)).is_err() {
                                return;
                            }
                        }
                        Ok(None) => break,
                        Err(e) => {
                            let _ = tx.send(UiEvent::Disconnected(e.to_string()));
                            break;
                        }
                    }
                }
                let _ = tx.send(UiEvent::Disconnected("daemon disconnected".into()));
            }
            Err(e) => {
                if tx.send(UiEvent::Disconnected(e)).is_err() {
                    return;
                }
            }
        }
        thread::sleep(Duration::from_secs(2));
    }
}

fn worker_loop(rx: Receiver<Request>, tx: Sender<UiEvent>) {
    while let Ok(req) = rx.recv() {
        let event = match request(req) {
            Ok(Event::Error { message }) => UiEvent::Error(message),
            Ok(event) => UiEvent::Event(event),
            Err(e) => UiEvent::Error(e),
        };
        if tx.send(event).is_err() {
            return;
        }
    }
}

type Conn = (UnixStream, BufReader<UnixStream>);

fn connect() -> Result<Conn, String> {
    let stream =
        UnixStream::connect(socket_path()).map_err(|e| format!("wacomd is not running: {e}"))?;
    let writer = stream.try_clone().map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream);
    match read_message::<_, Event>(&mut reader).map_err(|e| e.to_string())? {
        Some(Event::Hello { version }) if version == PROTOCOL_VERSION => Ok((writer, reader)),
        Some(Event::Hello { version }) => Err(format!("protocol mismatch: daemon v{version}")),
        _ => Err("unexpected greeting from daemon".into()),
    }
}

pub fn request(request: Request) -> Result<Event, String> {
    let (mut writer, mut reader) = connect()?;
    write_message(&mut writer, &request).map_err(|e| e.to_string())?;
    loop {
        match read_message::<_, Event>(&mut reader).map_err(|e| e.to_string())? {
            Some(Event::Telemetry(_)) | Some(Event::Pad { .. }) | Some(Event::Radial) => continue,
            Some(Event::Error { message }) => return Err(message),
            Some(event) => return Ok(event),
            None => return Err("daemon closed the connection".into()),
        }
    }
}
