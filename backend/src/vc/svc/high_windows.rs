use crate::vc;
use std::{ffi, io, ops, os, pin, thread};
use windows_sys as ws;

// VMware's `vdp_rdpvcbridge.dll` exports `VDP_VirtualChannel*` as plain C++
// functions, hence `__cdecl`: the *caller* cleans the stack. Citrix's `WFAPI`
// and the WTS ones are `WINAPI`, i.e. `__stdcall`: the *callee* cleans it.
//
// Mixing the two up does not break the call itself: the arguments are pushed
// the same way and the return value comes back in `%eax`. What breaks is
// `%esp`, which ends up off by the size of the arguments. rustc keeps a frame
// pointer on `i686-pc-windows-gnu`, so the code following the call keeps
// working (locals are addressed through `%ebp`) and the crash only shows up
// later. The epilogue is a bare `pop %ebp; ret` with no `mov %esp, %ebp`
// beforehand, so the saved frame pointer and the return address are then read
// from the wrong place and the process dies with an access violation exactly
// as `open()` returns.
#[cfg(feature = "svc")]
type VirtualChannelOpenCdecl = unsafe extern "C" fn(
    hserver: ws::Win32::Foundation::HANDLE,
    sessionid: os::raw::c_uint,
    pvirtualname: *mut os::raw::c_char,
) -> ws::Win32::Foundation::HANDLE;

#[cfg(feature = "svc")]
type VirtualChannelReadCdecl = unsafe extern "C" fn(
    hchannelhandle: ws::Win32::Foundation::HANDLE,
    timeout: os::raw::c_ulong,
    buffer: *mut os::raw::c_uchar,
    buffersize: os::raw::c_ulong,
    pbytesread: *mut os::raw::c_ulong,
) -> ws::core::BOOL;

#[cfg(feature = "svc")]
type VirtualChannelWriteCdecl = unsafe extern "C" fn(
    hchannelhandle: ws::Win32::Foundation::HANDLE,
    buffer: *const os::raw::c_uchar,
    length: os::raw::c_ulong,
    pbyteswritten: *mut os::raw::c_ulong,
) -> ws::core::BOOL;

#[cfg(feature = "svc")]
type VirtualChannelCloseCdecl =
    unsafe extern "C" fn(hchannelhandle: ws::Win32::Foundation::HANDLE) -> ws::core::BOOL;

/// Symbols of a "high" static virtual channel implementation.
///
/// The two variants carry the same functions and only differ by their calling
/// convention: `WFVirtualChannel*` is declared `WFAPI` (i.e. `__stdcall`, the
/// callee cleans the stack) while `VDP_VirtualChannel*` is a plain C export
/// (i.e. `__cdecl`, the caller cleans it). See the note on the `...Cdecl` types
/// in `crate::vc` for why confusing the two is fatal on `i686-pc-windows-gnu`.
pub enum Svc<'a> {
    Standard {
        open: libloading::Symbol<'a, super::VirtualChannelOpen>,
        read: libloading::Symbol<'a, vc::VirtualChannelRead>,
        write: libloading::Symbol<'a, vc::VirtualChannelWrite>,
        close: libloading::Symbol<'a, vc::VirtualChannelClose>,
    },
    HorizonSpecific {
        open: libloading::Symbol<'a, VirtualChannelOpenCdecl>,
        read: libloading::Symbol<'a, VirtualChannelReadCdecl>,
        write: libloading::Symbol<'a, VirtualChannelWriteCdecl>,
        close: libloading::Symbol<'a, VirtualChannelCloseCdecl>,
    },
}

impl<'a> vc::VirtualChannel<'a> for Svc<'a> {
    type Handle = Handle<'a>;

    fn load(libs: &'a vc::Libraries) -> Result<Self, vc::Error> {
        if let Some(citrix) = libs.citrix() {
            unsafe {
                Ok(Self::Standard {
                    open: citrix.get(b"WFVirtualChannelOpen")?,
                    read: citrix.get(b"WFVirtualChannelRead")?,
                    write: citrix.get(b"WFVirtualChannelWrite")?,
                    close: citrix.get(b"WFVirtualChannelClose")?,
                })
            }
        } else if let Some(horizon) = libs.horizon() {
            unsafe {
                Ok(Self::HorizonSpecific {
                    open: horizon.get(b"VDP_VirtualChannelOpen")?,
                    read: horizon.get(b"VDP_VirtualChannelRead")?,
                    write: horizon.get(b"VDP_VirtualChannelWrite")?,
                    close: horizon.get(b"VDP_VirtualChannelClose")?,
                })
            }
        } else {
            Err(vc::Error::NoLibraryFound)
        }
    }

    fn open(&'a self, name: [ffi::c_char; 8]) -> Result<Self::Handle, vc::Error> {
        common::debug!("trying to open SVC(high)");

        #[cfg(not(feature = "log"))]
        let display_name = None;

        #[cfg(feature = "log")]
        let display_name = Some(format!("SVC(high) {:?}", unsafe {
            ffi::CStr::from_ptr(name.as_ptr())
        }));

        let mut name = pin::Pin::new(Box::new(name));

        let wtshandle = unsafe {
            match &self {
                Self::Standard { open, .. } => (open)(
                    ws::Win32::System::RemoteDesktop::WTS_CURRENT_SERVER_HANDLE,
                    ws::Win32::System::RemoteDesktop::WTS_CURRENT_SESSION,
                    name.as_mut_ptr(),
                ),
                Self::HorizonSpecific { open, .. } => (open)(
                    ws::Win32::System::RemoteDesktop::WTS_CURRENT_SERVER_HANDLE,
                    ws::Win32::System::RemoteDesktop::WTS_CURRENT_SESSION,
                    name.as_mut_ptr(),
                ),
            }
        };

        common::debug!("SVC(high) open returned {wtshandle:?}");

        if wtshandle.is_null() {
            let err = io::Error::last_os_error();
            return Err(vc::Error::OpenChannelFailed(err.to_string()));
        }

        // NOTE: there used to be a `VirtualChannelQuery(WTSVirtualClientData)`
        // call here, inherited from the Citrix implementation. Its result was
        // never used (only a warning was logged on failure), but VMware's
        // `VDP_VirtualChannelQuery` crashes the whole process when called that
        // way, right after a successful `VDP_VirtualChannelOpen`. Since the
        // high implementations read and write through the handle directly
        // (unlike the low one, which needs `WTSVirtualFileHandle` to duplicate
        // the handle), the query is simply dropped.

        Ok(Handle {
            display_name,
            wtshandle,
            _name_holder: name,
            svc: self,
        })
    }
}

pub struct Handle<'a> {
    display_name: Option<String>,
    wtshandle: ws::Win32::Foundation::HANDLE,
    _name_holder: pin::Pin<Box<[ffi::c_char; 8]>>,
    svc: &'a Svc<'a>,
}

// Because of the *mut content (handle) Rust does not derive Send and
// Sync. Since we know how those data will be used (especially in
// terms of concurrency) we assume to unsafely implement Send and
// Sync.
unsafe impl Send for Handle<'_> {}
unsafe impl Sync for Handle<'_> {}

impl vc::Handle for Handle<'_> {
    fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }

    fn read(&self, data: &mut [u8]) -> Result<ops::Range<usize>, vc::Error> {
        let to_read = os::raw::c_ulong::try_from(data.len())
            .map_err(|e| vc::Error::ReadFailed(e.to_string()))?;

        let timeout = os::raw::c_ulong::MAX;

        let mut read = 0;

        let ret = unsafe {
            match &self.svc {
                Svc::Standard { read: read_fn, .. } => (read_fn)(
                    self.wtshandle,
                    timeout,
                    data.as_mut_ptr(),
                    to_read,
                    &raw mut read,
                ),
                Svc::HorizonSpecific { read: read_fn, .. } => (read_fn)(
                    self.wtshandle,
                    timeout,
                    data.as_mut_ptr(),
                    to_read,
                    &raw mut read,
                ),
            }
        };

        if ret == ws::Win32::Foundation::FALSE {
            let err = io::Error::last_os_error();
            Err(vc::Error::ReadFailed(err.to_string()))
        } else {
            let read = usize::try_from(read).map_err(|e| vc::Error::ReadFailed(e.to_string()))?;
            Ok(0..read)
        }
    }

    fn write(&self, data: &[u8]) -> Result<usize, vc::Error> {
        let to_write = os::raw::c_ulong::try_from(data.len())
            .map_err(|e| vc::Error::WriteFailed(e.to_string()))?;

        let mut written = 0;

        common::trace!("write {to_write} bytes");

        loop {
            let ret = unsafe {
                match &self.svc {
                    Svc::Standard { write, .. } => {
                        (write)(self.wtshandle, data.as_ptr(), to_write, &raw mut written)
                    }
                    Svc::HorizonSpecific { write, .. } => {
                        (write)(self.wtshandle, data.as_ptr(), to_write, &raw mut written)
                    }
                }
            };

            if ret == ws::Win32::Foundation::FALSE || written != to_write {
                if written == 0 {
                    common::trace!("send buffer seems full, yield now");
                    thread::yield_now();
                    continue;
                }
                if written != to_write {
                    common::error!("partial write: {written} / {to_write}");
                }
                let err = io::Error::last_os_error();
                return Err(vc::Error::WriteFailed(err.to_string()));
            }

            return usize::try_from(written).map_err(|e| vc::Error::WriteFailed(e.to_string()));
        }
    }

    fn close(self) -> Result<(), vc::Error> {
        let ret = unsafe {
            match &self.svc {
                Svc::Standard { close, .. } => (close)(self.wtshandle),
                Svc::HorizonSpecific { close, .. } => (close)(self.wtshandle),
            }
        };
        if ret == ws::Win32::Foundation::FALSE {
            let err = io::Error::last_os_error();
            Err(vc::Error::CloseChannelFailed(err.to_string()))
        } else {
            Ok(())
        }
    }
}
