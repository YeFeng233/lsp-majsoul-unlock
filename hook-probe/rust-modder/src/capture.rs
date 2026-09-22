//! Bounded, nonblocking copy of original frames to the on-device AI service.
//! No model, protobuf decoding, disk writes or inference run in the game process.
#[cfg(target_os = "android")]
mod android {
    use std::{
        io::Write,
        net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream},
        sync::{
            Mutex, OnceLock,
            atomic::{AtomicBool, AtomicU64, Ordering},
            mpsc::{self, SyncSender},
        },
        time::Duration,
    };

    const MAX_FRAME: usize = 1024 * 1024;
    const TOKEN_SIZE: usize = 32;
    static CONNECTED: AtomicBool = AtomicBool::new(false);
    static GENERATION: AtomicU64 = AtomicU64::new(1);
    static CONFIG_REVISION: AtomicU64 = AtomicU64::new(0);
    static ENDPOINT: OnceLock<Mutex<Option<Endpoint>>> = OnceLock::new();
    static SENDER: OnceLock<Mutex<SyncSender<Frame>>> = OnceLock::new();

    #[derive(Clone, PartialEq, Eq)]
    struct Endpoint {
        port: u16,
        token: [u8; TOKEN_SIZE],
    }

    struct Frame {
        connection: u64,
        kind: u8,
        data: Vec<u8>,
    }

    fn endpoint_slot() -> &'static Mutex<Option<Endpoint>> {
        ENDPOINT.get_or_init(|| Mutex::new(None))
    }

    pub fn configure() {
        if SENDER.get().is_some() {
            return;
        }
        let (tx, rx) = mpsc::sync_channel::<Frame>(32);
        if SENDER.set(Mutex::new(tx)).is_err() {
            return;
        }
        let _ = std::thread::Builder::new()
            .name("majmax-ai-copy".into())
            .spawn(move || {
                let mut socket: Option<TcpStream> = None;
                let mut active_revision = u64::MAX;
                loop {
                    let revision = CONFIG_REVISION.load(Ordering::SeqCst);
                    if revision != active_revision {
                        socket = None;
                        CONNECTED.store(false, Ordering::SeqCst);
                        while rx.try_recv().is_ok() {}
                        active_revision = revision;
                    }

                    if socket.is_none() {
                        let endpoint = endpoint_slot().lock().ok().and_then(|value| value.clone());
                        let Some(endpoint) = endpoint else {
                            std::thread::sleep(Duration::from_millis(500));
                            continue;
                        };
                        let address =
                            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, endpoint.port));
                        match TcpStream::connect_timeout(&address, Duration::from_millis(800)) {
                            Ok(mut stream) => {
                                let ready = stream.write_all(&endpoint.token).and_then(|_| {
                                    stream.set_write_timeout(Some(Duration::from_millis(250)))
                                });
                                if ready.is_err() {
                                    std::thread::sleep(Duration::from_millis(500));
                                    continue;
                                }
                                socket = Some(stream);
                                CONNECTED.store(true, Ordering::SeqCst);
                            }
                            Err(_) => {
                                std::thread::sleep(Duration::from_secs(1));
                                continue;
                            }
                        }
                    }

                    let frame = match rx.recv_timeout(Duration::from_secs(1)) {
                        Ok(frame) => frame,
                        Err(mpsc::RecvTimeoutError::Timeout) => Frame {
                            connection: 0,
                            kind: 4,
                            data: Vec::new(),
                        },
                        Err(_) => break,
                    };
                    if CONFIG_REVISION.load(Ordering::SeqCst) != active_revision {
                        continue;
                    }
                    if let Some(stream) = socket.as_mut() {
                        if write_frame(stream, &frame).is_err() {
                            CONNECTED.store(false, Ordering::SeqCst);
                            GENERATION.fetch_add(1, Ordering::SeqCst);
                            socket = None;
                        }
                    }
                }
            });
    }

    /// Installs or revokes the session endpoint delivered by the UID-checked
    /// Android ContentProvider. The bearer token is never written to disk/logs.
    pub fn set_endpoint(port: u16, token: &[u8]) {
        let next = if port != 0 && token.len() == TOKEN_SIZE {
            let mut secret = [0u8; TOKEN_SIZE];
            secret.copy_from_slice(token);
            Some(Endpoint {
                port,
                token: secret,
            })
        } else {
            None
        };
        let Ok(mut current) = endpoint_slot().lock() else {
            return;
        };
        if *current == next {
            return;
        }
        *current = next;
        CONFIG_REVISION.fetch_add(1, Ordering::SeqCst);
        GENERATION.fetch_add(1, Ordering::SeqCst);
        CONNECTED.store(false, Ordering::SeqCst);
    }

    fn write_frame(stream: &mut TcpStream, frame: &Frame) -> std::io::Result<()> {
        // Network byte order: payload length, loss/reconnect generation,
        // opaque connection ID, direction (0 down / 1 up / 2 close / 4 ping).
        let mut header = [0u8; 21];
        header[..4].copy_from_slice(&(frame.data.len() as u32).to_be_bytes());
        header[4..12].copy_from_slice(&GENERATION.load(Ordering::SeqCst).to_be_bytes());
        header[12..20].copy_from_slice(&frame.connection.to_be_bytes());
        header[20] = frame.kind;
        stream.write_all(&header)?;
        stream.write_all(&frame.data)
    }

    pub fn publish(connection: usize, kind: u8, data: &[u8]) {
        if !CONNECTED.load(Ordering::Relaxed) {
            return;
        }
        if data.len() > MAX_FRAME {
            GENERATION.fetch_add(1, Ordering::SeqCst);
            return;
        }
        let Some(sender) = SENDER.get() else { return };
        // Serialize concurrent callbacks without ever waiting for the worker.
        let Ok(sender) = sender.try_lock() else {
            GENERATION.fetch_add(1, Ordering::SeqCst);
            return;
        };
        let frame = Frame {
            connection: connection as u64,
            kind,
            data: data.to_vec(),
        };
        if sender.try_send(frame).is_err() {
            GENERATION.fetch_add(1, Ordering::SeqCst);
        }
    }
}

#[cfg(target_os = "android")]
pub use android::{configure, publish, set_endpoint};
#[cfg(not(target_os = "android"))]
pub fn configure() {}
#[cfg(not(target_os = "android"))]
pub fn set_endpoint(_: u16, _: &[u8]) {}
#[cfg(not(target_os = "android"))]
pub fn publish(_: usize, _: u8, _: &[u8]) {}
