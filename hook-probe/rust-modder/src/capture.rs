//! Bounded, nonblocking copy of original frames to the on-device AI service.
//! No model, protobuf decoding, disk writes or inference in the game process.
#[cfg(target_os = "android")]
mod android {
    use std::{
        io::Write,
        os::{fd::AsRawFd, linux::net::SocketAddrExt, unix::net::{SocketAddr, UnixStream}},
        sync::{atomic::{AtomicBool, AtomicU64, Ordering}, mpsc::{self, SyncSender}, Mutex, OnceLock},
        time::Duration,
    };

    const MAX_FRAME: usize = 1024 * 1024;
    static CONNECTED: AtomicBool = AtomicBool::new(false);
    static GENERATION: AtomicU64 = AtomicU64::new(1);
    static SENDER: OnceLock<Mutex<SyncSender<Frame>>> = OnceLock::new();
    struct Frame { connection: u64, kind: u8, data: Vec<u8> }

    fn connect(uid: u32) -> std::io::Result<UnixStream> {
        let address = SocketAddr::from_abstract_name(b"majmax.local-ai.v1")?;
        let stream = UnixStream::connect_addr(&address)?;
        let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
        let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let result = unsafe { libc::getsockopt(stream.as_raw_fd(), libc::SOL_SOCKET,
            libc::SO_PEERCRED, (&mut credentials as *mut libc::ucred).cast(), &mut length) };
        if result != 0 || credentials.uid != uid {
            return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "unexpected AI service UID"));
        }
        stream.set_write_timeout(Some(Duration::from_millis(250)))?;
        Ok(stream)
    }

    fn write_frame(stream: &mut UnixStream, frame: &Frame) -> std::io::Result<()> {
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

    pub fn configure(uid: u32) {
        if uid == 0 || SENDER.get().is_some() { return; }
        let (tx, rx) = mpsc::sync_channel::<Frame>(32);
        if SENDER.set(Mutex::new(tx)).is_err() { return; }
        let _ = std::thread::Builder::new().name("majmax-ai-copy".into()).spawn(move || {
            let mut socket: Option<UnixStream> = None;
            loop {
                if socket.is_none() {
                    CONNECTED.store(false, Ordering::SeqCst);
                    while rx.try_recv().is_ok() {}
                    match connect(uid) {
                        Ok(stream) => {
                            GENERATION.fetch_add(1, Ordering::SeqCst);
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
                    Err(mpsc::RecvTimeoutError::Timeout) => Frame { connection: 0, kind: 4, data: Vec::new() },
                    Err(_) => break,
                };
                if let Some(stream) = socket.as_mut() {
                    if write_frame(stream, &frame).is_err() {
                        CONNECTED.store(false, Ordering::SeqCst);
                        socket = None;
                    }
                }
            }
        });
    }

    pub fn publish(connection: usize, kind: u8, data: &[u8]) {
        if !CONNECTED.load(Ordering::Relaxed) { return; }
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
        let frame = Frame { connection: connection as u64, kind, data: data.to_vec() };
        if sender.try_send(frame).is_err() { GENERATION.fetch_add(1, Ordering::SeqCst); }
    }
}

#[cfg(target_os = "android")]
pub use android::{configure, publish};
#[cfg(not(target_os = "android"))]
pub fn configure(_: u32) {}
#[cfg(not(target_os = "android"))]
pub fn publish(_: usize, _: u8, _: &[u8]) {}
