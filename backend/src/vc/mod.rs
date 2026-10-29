#[cfg(not(any(feature = "dvc", feature = "svc")))]
compile_error! {"feature \"dvc\" and/or feature \"svc\" must be enabled"}

use std::{ffi, fmt, ops, os};
use windows_sys as ws;

#[cfg(feature = "dvc")]
mod dvc;
#[cfg(feature = "svc")]
mod svc;

type VirtualChannelQuery = unsafe extern "system" fn(
    hchannelhandle: ws::Win32::Foundation::HANDLE,
    wtsvirtualclass: ws::Win32::System::RemoteDesktop::WTS_VIRTUAL_CLASS,
    ppbuffer: *mut *mut os::raw::c_void,
    pbytesreturned: *mut os::raw::c_ulong,
) -> ws::core::BOOL;

type VirtualChannelRead = unsafe extern "system" fn(
    hchannelhandle: ws::Win32::Foundation::HANDLE,
    timeout: os::raw::c_ulong,
    buffer: *mut os::raw::c_uchar,
    buffersize: os::raw::c_ulong,
    pbytesread: *mut os::raw::c_ulong,
) -> ws::core::BOOL;

type VirtualChannelWrite = unsafe extern "system" fn(
    hchannelhandle: ws::Win32::Foundation::HANDLE,
    buffer: *const os::raw::c_uchar,
    length: os::raw::c_ulong,
    pbyteswritten: *mut os::raw::c_ulong,
) -> ws::core::BOOL;

type VirtualChannelClose =
    unsafe extern "system" fn(hchannelhandle: ws::Win32::Foundation::HANDLE) -> ws::core::BOOL;

pub enum Error {
    NoLibraryFound,
    LibraryLoading(libloading::Error),
    #[cfg(target_os = "windows")]
    WsaStartupFailed(i32),
    OpenChannelFailed(String),
    CloseChannelFailed(String),
    ReadFailed(String),
    WriteFailed(String),
    #[cfg(target_os = "windows")]
    QueryFailed(String),
    #[cfg(target_os = "windows")]
    DuplicateHandleFailed(String),
    #[cfg(target_os = "windows")]
    CreateEventFailed(String),
}

impl From<libloading::Error> for Error {
    fn from(e: libloading::Error) -> Self {
        Self::LibraryLoading(e)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match self {
            Self::NoLibraryFound => write!(f, "no library found"),
            Self::LibraryLoading(e) => write!(f, "library loading error: {e}"),
            #[cfg(target_os = "windows")]
            Self::WsaStartupFailed(e) => write!(f, "WSAStartup failed with error code {e}"),
            Self::OpenChannelFailed(err) => {
                write!(f, "open failed (last_error = {err})")
            }
            Self::CloseChannelFailed(err) => {
                write!(f, "close failed (last_error = {err})")
            }
            Self::ReadFailed(err) => {
                write!(f, "read failed (last error = {err})")
            }
            Self::WriteFailed(err) => {
                write!(f, "write failed (last error = {err})")
            }
            #[cfg(target_os = "windows")]
            Self::QueryFailed(err) => {
                write!(f, "query failed (last error = {err})")
            }
            #[cfg(target_os = "windows")]
            Self::DuplicateHandleFailed(err) => {
                write!(f, "duplicate handle failed (last error = {err})")
            }
            #[cfg(target_os = "windows")]
            Self::CreateEventFailed(err) => {
                write!(f, "create event failed (last error = {err})")
            }
        }
    }
}

// Citrix ships one "Window Flow" API library per architecture: `wfapi64.dll`
// for 64 bits processes and `wfapi32.dll` for 32 bits ones. A 32 bits process
// cannot load the 64 bits library.
#[cfg(all(target_os = "windows", target_pointer_width = "32"))]
const CITRIX_LIBRARIES: [&str; 1] = ["wfapi32"];
#[cfg(all(target_os = "windows", target_pointer_width = "64"))]
const CITRIX_LIBRARIES: [&str; 2] = ["wfapi64", "wfapi32"];

pub struct Libraries {
    #[cfg(all(feature = "svc", target_os = "windows"))]
    vdp_rdpvcbridge: Option<libloading::Library>,
    #[cfg(feature = "svc")]
    citrix: Option<libloading::Library>,
    #[cfg(target_os = "windows")]
    wtsapi32: Option<libloading::Library>,
    xrdpapi: Option<libloading::Library>,
}

impl Libraries {
    #[cfg(all(feature = "svc", target_os = "windows"))]
    pub const fn horizon(&self) -> Option<&libloading::Library> {
        self.vdp_rdpvcbridge.as_ref()
    }

    #[cfg(feature = "svc")]
    pub const fn citrix(&self) -> Option<&libloading::Library> {
        self.citrix.as_ref()
    }

    #[cfg(target_os = "windows")]
    pub const fn wts(&self) -> Option<&libloading::Library> {
        self.wtsapi32.as_ref()
    }

    pub const fn xrdp(&self) -> Option<&libloading::Library> {
        self.xrdpapi.as_ref()
    }

    pub fn load() -> Self {
        unsafe {
            common::trace!("trying to load Citrix library");
            #[cfg(target_os = "windows")]
            let citrix = CITRIX_LIBRARIES
                .iter()
                .find_map(|name| libloading::Library::new(libloading::library_filename(name)).ok());
            #[cfg(not(target_os = "windows"))]
            let citrix = libloading::Library::new(libloading::library_filename("winsta")).ok();

            #[cfg(all(feature = "svc", target_os = "windows"))]
            let vdp_rdpvcbridge = {
                common::trace!("trying to load Horizon library");
                libloading::Library::new(libloading::library_filename("vdp_rdpvcbridge")).ok()
            };

            common::trace!("trying to load XRDP library");
            let xrdpapi = libloading::Library::new(libloading::library_filename("xrdpapi")).ok();

            #[cfg(target_os = "windows")]
            let wtsapi32 = {
                common::trace!("trying to load WTS library");
                libloading::Library::new(libloading::library_filename("wtsapi32")).ok()
            };

            #[cfg(feature = "log")]
            {
                let found: Vec<&str> = vec![
                    citrix.as_ref().map(|_| "Citrix"),
                    #[cfg(all(feature = "svc", target_os = "windows"))]
                    vdp_rdpvcbridge.as_ref().map(|_| "Horizon"),
                    xrdpapi.as_ref().map(|_| "XRDP"),
                    #[cfg(target_os = "windows")]
                    wtsapi32.as_ref().map(|_| "WTS"),
                ]
                .into_iter()
                .flatten()
                .collect();

                common::info!("found libraries: {}", found.join(", "));
            }

            Self {
                #[cfg(all(feature = "svc", target_os = "windows"))]
                vdp_rdpvcbridge,
                #[cfg(feature = "svc")]
                citrix,
                #[cfg(target_os = "windows")]
                wtsapi32,
                xrdpapi,
            }
        }
    }
}

pub trait VirtualChannel<'a>: Sized + Send + Sync {
    type Handle: Handle;
    fn load(libs: &'a Libraries) -> Result<Self, Error>;
    fn open(&'a self, name: [ffi::c_char; 8]) -> Result<Self::Handle, Error>;
}

pub struct GenericChannel<'a> {
    #[cfg(feature = "dvc")]
    dvc: Option<dvc::Dvc<'a>>,
    #[cfg(feature = "svc")]
    svc: Option<svc::Svc<'a>>,
}

impl<'a> VirtualChannel<'a> for GenericChannel<'a> {
    type Handle = GenericHandle<'a>;

    fn load(libs: &'a Libraries) -> Result<Self, Error> {
        #[cfg(not(feature = "dvc"))]
        let dvc: Option<()> = None;
        #[cfg(feature = "dvc")]
        let dvc = dvc::Dvc::load(libs)
            .inspect_err(|e| {
                common::warn!("no DVC loaded: {e}");
            })
            .ok();

        #[cfg(not(feature = "svc"))]
        let svc: Option<()> = None;
        #[cfg(feature = "svc")]
        let svc = svc::Svc::load(libs)
            .inspect_err(|e| {
                common::warn!("no SVC loaded: {e}");
            })
            .ok();

        if svc.is_none() && dvc.is_none() {
            return Err(Error::NoLibraryFound);
        }

        Ok(Self {
            #[cfg(feature = "dvc")]
            dvc,
            #[cfg(feature = "svc")]
            svc,
        })
    }

    fn open(&'a self, name: [ffi::c_char; 8]) -> Result<Self::Handle, Error> {
        #[cfg(not(feature = "svc"))]
        let svc: Result<Self::Handle, _> = Err(Error::NoLibraryFound);
        #[cfg(feature = "svc")]
        let svc = self
            .svc
            .as_ref()
            .ok_or(Error::NoLibraryFound)
            .and_then(|svc| svc.open(name))
            .map(GenericHandle::Static)
            .inspect_err(|e| common::error!("failed to open SVC: {e}"));

        #[cfg(not(feature = "dvc"))]
        let dvc: Result<Self::Handle, _> = Err(Error::NoLibraryFound);
        #[cfg(feature = "dvc")]
        let dvc = self
            .dvc
            .as_ref()
            .ok_or(Error::NoLibraryFound)
            .and_then(|dvc| dvc.open(name))
            .map(GenericHandle::Dynamic)
            .inspect_err(|e| common::error!("failed to open DVC: {e}"));

        dvc.or(svc)
    }
}

pub trait Handle: Send + Sync {
    fn display_name(&self) -> Option<&str>;
    fn read(&self, data: &mut [u8]) -> Result<ops::Range<usize>, Error>;
    fn write(&self, data: &[u8]) -> Result<usize, Error>;
    fn close(self) -> Result<(), Error>;
}

pub enum GenericHandle<'a> {
    #[cfg(feature = "dvc")]
    Dynamic(dvc::Handle<'a>),
    #[cfg(feature = "svc")]
    Static(svc::Handle<'a>),
}

impl Handle for GenericHandle<'_> {
    fn display_name(&self) -> Option<&str> {
        match self {
            #[cfg(feature = "dvc")]
            Self::Dynamic(dvch) => dvch.display_name(),
            #[cfg(feature = "svc")]
            Self::Static(svch) => svch.display_name(),
        }
    }

    fn read(&self, data: &mut [u8]) -> Result<ops::Range<usize>, Error> {
        match self {
            #[cfg(feature = "dvc")]
            Self::Dynamic(dvch) => dvch.read(data),
            #[cfg(feature = "svc")]
            Self::Static(svch) => svch.read(data),
        }
    }

    fn write(&self, data: &[u8]) -> Result<usize, Error> {
        match self {
            #[cfg(feature = "dvc")]
            Self::Dynamic(dvch) => dvch.write(data),
            #[cfg(feature = "svc")]
            Self::Static(svch) => svch.write(data),
        }
    }

    fn close(self) -> Result<(), Error> {
        match self {
            #[cfg(feature = "dvc")]
            Self::Dynamic(dvch) => dvch.close(),
            #[cfg(feature = "svc")]
            Self::Static(svch) => svch.close(),
        }
    }
}
