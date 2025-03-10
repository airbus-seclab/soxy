use common::{self, api, service};
use std::{ffi, fmt, ptr, sync, thread, time};
use svc::Handler;
use windows_sys as ws;

mod svc;

const TO_SVC_CHANNEL_SIZE: usize = 256;

enum Error {
    Svc(svc::Error),
    PipelineBroken,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match self {
            Self::Svc(e) => write!(f, "virtual channel error: {e}"),
            Self::PipelineBroken => write!(f, "broken pipeline"),
        }
    }
}

impl From<svc::Error> for Error {
    fn from(e: svc::Error) -> Self {
        Self::Svc(e)
    }
}

impl From<crossbeam_channel::RecvError> for Error {
    fn from(_e: crossbeam_channel::RecvError) -> Self {
        Self::PipelineBroken
    }
}

impl<T> From<crossbeam_channel::SendError<T>> for Error {
    fn from(_e: crossbeam_channel::SendError<T>) -> Self {
        Self::PipelineBroken
    }
}

fn backend_to_frontend(
    channel: &sync::RwLock<Option<svc::Handle<'_>>>,
    from_backend: &crossbeam_channel::Receiver<api::ChunkControl>,
) -> Result<(), Error> {
    let mut disconnect = false;

    loop {
        match from_backend.recv()? {
            api::ChunkControl::Shutdown => {
                common::info!("received shutdown, closing");
                disconnect = true;
            }
            api::ChunkControl::Chunk(chunk) => {
                common::trace!("{chunk}");

                let data = chunk.serialized();

                match channel.read().unwrap().as_ref() {
                    None => {
                        common::debug!("cannot write on disconnected channel");
                    }
                    Some(svc) => {
                        if let Err(e) = svc.write(&data) {
                            common::error!("failed to write on channel: {e}");
                            disconnect = true;
                        }
                    }
                }
            }
        }

        if disconnect {
            common::info!("disconnecting from channel");
            channel.write().unwrap().take();
            disconnect = false;
        }
    }
}

fn frontend_to_backend<'a>(
    svc: &'a svc::Svc<'a>,
    channel: &'a sync::RwLock<Option<svc::Handle<'a>>>,
    to_backend: &crossbeam_channel::Sender<api::ChunkControl>,
) -> Result<(), Error> {
    let mut connect = true;
    let mut disconnect = false;

    let mut buf = [0u8; api::Chunk::serialized_overhead() + api::Chunk::max_payload_length()];

    loop {
        if connect {
            common::debug!("open static channel {:?}", common::VIRTUAL_CHANNEL_NAME);
            match svc.open(common::VIRTUAL_CHANNEL_NAME) {
                Err(e) => {
                    common::error!("failed to open channel handle: {e}");
                    thread::sleep(time::Duration::from_secs(1));
                    continue;
                }
                Ok(svc_handle) => {
                    common::info!("static channel {:?} opened", common::VIRTUAL_CHANNEL_NAME);
                    channel.write().unwrap().replace(svc_handle);
                    connect = false;
                }
            }
        }

        match channel.read().unwrap().as_ref() {
            None => {
                common::debug!("cannot read on disconnected channel");
                connect = true;
            }
            Some(svc) => match svc.read(&mut buf) {
                Err(e) => {
                    common::error!("failed to read from channel: {e}");
                    disconnect = true;
                }
                Ok(read) => {
                    common::trace!("read = {:?}", &buf[0..read]);

                    match api::Chunk::deserialize(&buf[0..read]) {
                        Err(e) => {
                            common::error!("failed to deserialize chunk: {e}");
                            disconnect = true;
                        }
                        Ok(chunk) => {
                            common::trace!("{chunk}");
                            to_backend.send(api::ChunkControl::Chunk(chunk))?;
                        }
                    }
                }
            },
        }

        if disconnect {
            common::info!("disconnecting from channel");
            channel.write().unwrap().take();
            to_backend.send(api::ChunkControl::Shutdown)?;
            disconnect = false;
        }
    }
}

#[allow(clippy::too_many_lines)]
fn main_res() -> Result<(), Error> {
    common::debug!("calling WSAStartup");

    let mut data = ws::Win32::Networking::WinSock::WSADATA {
        wVersion: 0,
        wHighVersion: 0,
        iMaxSockets: 0,
        iMaxUdpDg: 0,
        lpVendorInfo: ptr::null_mut(),
        szDescription: [0i8; 257],
        szSystemStatus: [0i8; 129],
    };

    let ret = unsafe { ws::Win32::Networking::WinSock::WSAStartup(0x0202, &mut data) };
    if ret != 0 {
        return Err(Error::Svc(svc::Error::WsaStartupFailed(ret)));
    }

    let lib = svc::Implementation::load()?;
    let svc = svc::Svc::load(&lib)?;

    let (backend_to_frontend_send, backend_to_frontend_receive) =
        crossbeam_channel::bounded(TO_SVC_CHANNEL_SIZE);
    let (frontend_to_backend_send, frontend_to_backend_receive) = crossbeam_channel::unbounded();

    let backend_channel = service::Channel::new(backend_to_frontend_send);

    thread::Builder::new()
        .name("backend".into())
        .spawn(move || {
            if let Err(e) =
                backend_channel.start(api::ServiceKind::Backend, &frontend_to_backend_receive)
            {
                common::error!("error: {e}");
            } else {
                common::debug!("stopped");
            }
        })
        .unwrap();

    let channel = sync::RwLock::new(None);

    thread::scope(|scope| {
        thread::Builder::new()
            .name("backend to frontend".into())
            .spawn_scoped(scope, || {
                if let Err(e) = backend_to_frontend(&channel, &backend_to_frontend_receive) {
                    common::error!("error: {e}");
                } else {
                    common::warn!("stopped");
                }
            })
            .unwrap();

        if let Err(e) = frontend_to_backend(&svc, &channel, &frontend_to_backend_send) {
            common::error!("error: {e}");
            Err(e)
        } else {
            common::warn!("stopped");
            Ok(())
        }
    })
}

pub fn main() {
    common::init_logs(false);

    common::debug!("starting up");

    if let Err(e) = main_res() {
        common::error!("{e}");
    }
}

// The Main in only there to maintain the library loaded while loaded
// through rundll32.exe, which executes at loading time the DllMain
// function below. The DllMain function is called by the loader and
// must return ASAP to unlock the loading process. That is why we
// create a thread in it.

// rundll32.exe soxy.dll,Main

#[no_mangle]
#[allow(non_snake_case, unused_variables)]
pub extern "system" fn Main() {
    loop {
        thread::sleep(time::Duration::from_secs(60));
    }
}

#[no_mangle]
#[allow(non_snake_case, unused_variables, clippy::missing_safety_doc)]
pub unsafe extern "system" fn DllMain(
    dll_module: ws::Win32::Foundation::HINSTANCE,
    call_reason: u32,
    _reserverd: *mut ffi::c_void,
) -> ws::Win32::Foundation::BOOL {
    match call_reason {
        ws::Win32::System::SystemServices::DLL_PROCESS_ATTACH => unsafe {
            ws::Win32::System::LibraryLoader::DisableThreadLibraryCalls(dll_module);
            ws::Win32::System::Console::AllocConsole();
            thread::spawn(main);
        },
        ws::Win32::System::SystemServices::DLL_PROCESS_DETACH => {}
        _ => (),
    }

    ws::Win32::Foundation::TRUE
}
