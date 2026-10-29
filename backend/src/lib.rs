use common::{api, channel, service};
use std::{ffi, fmt, sync, thread, time};
#[cfg(any(feature = "dvc", feature = "svc"))]
use vc::{Handle, VirtualChannel};
use windows_sys as ws;

#[cfg(any(feature = "dvc", feature = "svc"))]
mod vc;

enum Error {
    Api(api::Error),
    #[cfg(any(feature = "dvc", feature = "svc"))]
    Vc(vc::Error),
    Crossbeam(String),
}

impl From<api::Error> for Error {
    fn from(e: api::Error) -> Self {
        Self::Api(e)
    }
}

#[cfg(any(feature = "dvc", feature = "svc"))]
impl From<vc::Error> for Error {
    fn from(e: vc::Error) -> Self {
        Self::Vc(e)
    }
}

impl From<crossbeam_channel::RecvError> for Error {
    fn from(e: crossbeam_channel::RecvError) -> Self {
        Self::Crossbeam(e.to_string())
    }
}

impl<T> From<crossbeam_channel::SendError<T>> for Error {
    fn from(e: crossbeam_channel::SendError<T>) -> Self {
        Self::Crossbeam(e.to_string())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match self {
            Self::Api(e) => write!(f, "API error: {e}"),
            Self::Vc(e) => write!(f, "virtual channel error: {e}"),
            Self::Crossbeam(msg) => write!(f, "internal error: {msg}"),
        }
    }
}

const TO_VC_CHANNEL_SIZE: usize = 64;

fn backend_to_frontend<H>(
    handle: &sync::RwLock<Option<H>>,
    from_backend: &crossbeam_channel::Receiver<api::Message>,
) -> Result<(), Error>
where
    H: vc::Handle,
{
    loop {
        match from_backend.recv()? {
            api::Message::Chunk(chunk) => {
                common::trace!("{chunk}");

                if matches!(
                    chunk.chunk_type()?,
                    api::ChunkType::Start | api::ChunkType::End
                ) {
                    common::debug!("BACKEND send {chunk}");
                }

                let data = chunk.serialized();

                match handle.read().unwrap().as_ref() {
                    None => {
                        common::warn!("disconnected channel, discarding chunk");
                    }
                    Some(handle) => {
                        let _ = handle.write(&data)?;
                    }
                }
            }
            #[cfg(feature = "service-input")]
            api::Message::InputSetting(_setting) => {
                common::debug!("discarding input setting");
            }
            #[cfg(feature = "service-input")]
            api::Message::InputAction(_action) => {
                common::debug!("discarding input action");
            }
            #[cfg(feature = "service-input")]
            api::Message::ResetClient => {
                common::debug!("discarding reset client");
            }
            api::Message::Shutdown => {
                common::debug!("received shutdown, closing");
                return Ok(());
            }
        }
    }
}

fn frontend_to_backend<'a, V>(
    channel_name: [ffi::c_char; 8],
    vc: &'a V,
    handle: &sync::RwLock<Option<V::Handle>>,
    to_backend: &crossbeam_channel::Sender<api::Message>,
) -> Result<(), Error>
where
    V: vc::VirtualChannel<'a>,
{
    let mut received_data = Vec::with_capacity(3 * api::PDU_DATA_MAX_SIZE);
    let mut buf = [0u8; 3 * api::PDU_MAX_SIZE];

    common::debug!("open virtual channel {channel_name:?}");

    let vchandle = vc.open(channel_name)?;

    common::info!(
        "virtual channel {} opened",
        vchandle.display_name().unwrap_or("<unknown>")
    );
    handle.write().unwrap().replace(vchandle);

    loop {
        match handle.read().unwrap().as_ref() {
            None => {
                common::debug!("internal disconnection");
                return Ok(());
            }
            Some(handle) => {
                common::trace!("READ max {} bytes", buf.len());

                let mut read = handle.read(&mut buf)?;

                if received_data.is_empty() {
                    'inner: loop {
                        match api::Chunk::can_deserialize_from(&buf[read.clone()]) {
                            None => {
                                received_data.extend_from_slice(&buf[read.clone()]);
                                break 'inner;
                            }
                            Some(len) => {
                                let chunk = api::Chunk::deserialize_from(
                                    &buf[read.start..read.start + len],
                                )?;
                                to_backend.send(api::Message::Chunk(chunk))?;

                                read.start += len;

                                if read.is_empty() {
                                    break 'inner;
                                }
                            }
                        }
                    }
                } else {
                    received_data.extend_from_slice(&buf[read]);

                    'inner: loop {
                        match api::Chunk::can_deserialize_from(&received_data) {
                            None => break 'inner,
                            Some(len) => {
                                let tail = received_data.split_off(len);
                                let head = received_data;
                                received_data = tail;

                                let chunk = api::Chunk::deserialize(head)?;
                                to_backend.send(api::Message::Chunk(chunk))?;
                            }
                        }
                    }
                }
            }
        }
    }
}

fn run<'a, V>(
    channel_name: [ffi::c_char; 8],
    vc: &'a V,
    handle: &sync::RwLock<Option<V::Handle>>,
    frontend_to_backend_send: &crossbeam_channel::Sender<api::Message>,
    backend_to_frontend_receive: &crossbeam_channel::Receiver<api::Message>,
) where
    V: vc::VirtualChannel<'a>,
{
    thread::scope(|scope| {
        let thread = thread::Builder::new();
        #[cfg(feature = "log")]
        let thread = thread.name("backend to frontend".into());
        thread
            .spawn_scoped(scope, || {
                if let Err(e) = backend_to_frontend(handle, backend_to_frontend_receive) {
                    common::error!("stopped: {e}");
                    if let Some(handle) = handle.write().unwrap().take()
                        && let Err(e) = handle.close()
                    {
                        common::warn!("failed to close channel: {e}");
                    }
                } else {
                    common::debug!("stopped");
                }
            })
            .unwrap();

        let thread = thread::Builder::new();
        #[cfg(feature = "log")]
        let thread = thread.name("frontend to backend".into());
        thread
            .spawn_scoped(scope, || {
                if let Err(e) =
                    frontend_to_backend(channel_name, vc, handle, frontend_to_backend_send)
                {
                    common::error!("stopped: {e}");
                    if let Some(handle) = handle.write().unwrap().take()
                        && let Err(e) = handle.close()
                    {
                        common::warn!("failed to close channel: {e}");
                    }
                    if let Err(e) = frontend_to_backend_send.send(api::Message::Shutdown) {
                        common::warn!("failed to send shutdown: {e}");
                    }
                } else {
                    common::debug!("stopped");
                }
            })
            .unwrap();
    });
}

#[allow(clippy::too_many_lines)]
fn main_res(channel_name: [ffi::c_char; 8]) -> Result<(), Error> {
    #[cfg(target_os = "windows")]
    {
        common::debug!("calling WSAStartup");

        let mut data = ws::Win32::Networking::WinSock::WSADATA {
            wVersion: 0,
            wHighVersion: 0,
            iMaxSockets: 0,
            iMaxUdpDg: 0,
            lpVendorInfo: std::ptr::null_mut(),
            szDescription: [0i8; 257],
            szSystemStatus: [0i8; 129],
        };

        let ret = unsafe { ws::Win32::Networking::WinSock::WSAStartup(0x0202, &raw mut data) };
        if ret != 0 {
            return Err(Error::Vc(vc::Error::WsaStartupFailed(ret)));
        }
    }

    let (backend_to_frontend_send, backend_to_frontend_receive) =
        crossbeam_channel::bounded(TO_VC_CHANNEL_SIZE);
    let (frontend_to_backend_send, frontend_to_backend_receive) = crossbeam_channel::unbounded();

    let backend_channel = channel::Channel::new(backend_to_frontend_send);

    let thread = thread::Builder::new();
    #[cfg(feature = "log")]
    let thread = thread.name("backend".into());
    thread.spawn(move || {
            #[cfg(target_os = "windows")]
            {
                let ret_exec = unsafe {
                    ws::Win32::System::Power::SetThreadExecutionState(
                        ws::Win32::System::Power::ES_CONTINUOUS
                            | ws::Win32::System::Power::ES_DISPLAY_REQUIRED
                            | ws::Win32::System::Power::ES_SYSTEM_REQUIRED,
                    )
                };

                if ret_exec == 0 {
                    common::warn!("failed to set thread ExecutionState with: ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED");
                }
            }
            if let Err(e) =
                backend_channel.run(service::Kind::Backend, &frontend_to_backend_receive)
            {
                common::error!("backend channel stopped: {e}");
            } else {
                common::debug!("backend channel stopped");
            }
        })
        .unwrap();

    // The VDI virtual channel library (Citrix `wfapi`, Horizon
    // `vdp_rdpvcbridge`, XRDP `xrdpapi`) is not necessarily loadable when soxy
    // starts: the agent providing it can be installed after us, and on Windows
    // it can even be missing until a session is established. Keep retrying
    // instead of bailing out: returning here makes the `soxy` binary exit
    // immediately, its console window flashing and closing (and the log along
    // with it), while the DLL flavour would just silently stop working.
    loop {
        let libs = vc::Libraries::load();

        let (channel, vc) = match vc::GenericChannel::load(&libs) {
            Ok(vc) => (sync::RwLock::new(None), vc),
            Err(e) => {
                common::error!("no virtual channel library loaded: {e}");
                common::error!("retrying in 2 seconds (is the VDI agent running?)");

                thread::sleep(time::Duration::from_secs(2));

                continue;
            }
        };

        loop {
            run(
                channel_name,
                &vc,
                &channel,
                &frontend_to_backend_send,
                &backend_to_frontend_receive,
            );

            thread::sleep(time::Duration::from_secs(2));
        }
    }
}

pub fn main(channel_name: &str, level: common::Level) {
    common::init_logs(level, None);

    common::log_package_infos!();

    common::info!("virtual channel name is {channel_name:?}");

    match common::virtual_channel_name(channel_name) {
        Err(e) => common::error!("{e}"),
        Ok(channel_name) => {
            common::debug!("starting up");

            if let Err(e) = main_res(channel_name) {
                common::error!("{e}");
            }
        }
    }
}

// The Main in only there to maintain the library loaded while loaded
// through rundll32.exe, which executes at loading time the DllMain
// function below. The DllMain function is called by the loader and
// must return ASAP to unlock the loading process. That is why we
// create a thread in it.

// rundll32.exe soxy.dll,Main

#[unsafe(no_mangle)]
#[allow(non_snake_case, unused_variables)]
extern "system" fn Main() {
    loop {
        thread::sleep(time::Duration::from_mins(1));
    }
}

#[unsafe(no_mangle)]
#[allow(non_snake_case, unused_variables, clippy::missing_safety_doc)]
pub unsafe extern "system" fn DllMain(
    dll_module: ws::Win32::Foundation::HINSTANCE,
    call_reason: u32,
    _reserved: *mut ffi::c_void,
) -> ws::core::BOOL {
    match call_reason {
        ws::Win32::System::SystemServices::DLL_PROCESS_ATTACH => unsafe {
            ws::Win32::System::LibraryLoader::DisableThreadLibraryCalls(dll_module);
            ws::Win32::System::Console::AllocConsole();
            thread::spawn(|| {
                #[cfg(debug_assertions)]
                main(common::VIRTUAL_CHANNEL_DEFAULT_NAME, common::Level::Debug);
                #[cfg(not(debug_assertions))]
                main(common::VIRTUAL_CHANNEL_DEFAULT_NAME, common::Level::Info);
            });
        },
        ws::Win32::System::SystemServices::DLL_PROCESS_DETACH => {}
        _ => (),
    }

    ws::Win32::Foundation::TRUE
}
