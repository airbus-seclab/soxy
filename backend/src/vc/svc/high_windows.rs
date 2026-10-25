use crate::vc;
use std::{ffi, io, ops, os, thread};
use windows_sys as ws;

/// Symbols of a "high" static virtual channel implementation.
///
/// The two variants carry the same functions and only differ by their calling
/// convention: `WFVirtualChannel*` is declared `WFAPI` (i.e. `__stdcall`, the
/// callee cleans the stack) while `VDP_VirtualChannel*` is a plain C export
/// (i.e. `__cdecl`, the caller cleans it). See the note on the `...Cdecl` types
/// in `crate::vc` for why confusing the two is fatal on `i686-pc-windows-gnu`.
#[derive(Clone)]
enum Syms<'a> {
    Stdcall {
        open: libloading::Symbol<'a, vc::VirtualChannelOpen>,
        read: libloading::Symbol<'a, vc::VirtualChannelRead>,
        write: libloading::Symbol<'a, vc::VirtualChannelWrite>,
        close: libloading::Symbol<'a, vc::VirtualChannelClose>,
    },
    Cdecl {
        open: libloading::Symbol<'a, vc::VirtualChannelOpenCdecl>,
        read: libloading::Symbol<'a, vc::VirtualChannelReadCdecl>,
        write: libloading::Symbol<'a, vc::VirtualChannelWriteCdecl>,
        close: libloading::Symbol<'a, vc::VirtualChannelCloseCdecl>,
    },
}

pub struct Svc<'a> {
    syms: Syms<'a>,
}

impl<'a> vc::VirtualChannel<'a> for Svc<'a> {
    type Handle = Handle<'a>;

    fn load(libs: &'a vc::Libraries) -> Result<Self, vc::Error> {
        if let Some(citrix) = libs.citrix() {
            unsafe {
                Ok(Self {
                    syms: Syms::Stdcall {
                        open: citrix.get(b"WFVirtualChannelOpen")?,
                        read: citrix.get(b"WFVirtualChannelRead")?,
                        write: citrix.get(b"WFVirtualChannelWrite")?,
                        close: citrix.get(b"WFVirtualChannelClose")?,
                    },
                })
            }
        } else if let Some(horizon) = libs.horizon() {
            unsafe {
                Ok(Self {
                    syms: Syms::Cdecl {
                        open: horizon.get(b"VDP_VirtualChannelOpen")?,
                        read: horizon.get(b"VDP_VirtualChannelRead")?,
                        write: horizon.get(b"VDP_VirtualChannelWrite")?,
                        close: horizon.get(b"VDP_VirtualChannelClose")?,
                    },
                })
            }
        } else {
            Err(vc::Error::NoLibraryFound)
        }
    }

    fn open(&self, name: [ffi::c_char; 8]) -> Result<Self::Handle, vc::Error> {
        common::debug!("trying to open SVC(high)");

        // The channel name is handed over as a *mut buffer, and the RDP virtual
        // channel APIs treat it as a fixed 8 bytes field which is not required to
        // be NUL terminated (VMware rewrites it its own way). The display name
        // must therefore be computed *before* the call: reading the buffer back
        // afterwards with `CStr::from_ptr` walks past the end of the array.
        #[cfg(feature = "log")]
        let display_name = unsafe { ffi::CStr::from_ptr(name.as_ptr()) }
            .to_str()
            .ok()
            .map(|n| format!("SVC(high) {n:?}"));
        #[cfg(not(feature = "log"))]
        let display_name: Option<String> = None;

        // The channel name is handed over as a *mut buffer: give the library
        // room on the heap rather than a `[c_char; 8]` sitting on the stack.
        let mut name_buf = vec![0i8; name.len() + 248];
        name_buf[..name.len()].copy_from_slice(&name);

        let wtshandle = unsafe {
            match &self.syms {
                Syms::Stdcall { open, .. } => (open)(
                    ws::Win32::System::RemoteDesktop::WTS_CURRENT_SERVER_HANDLE,
                    ws::Win32::System::RemoteDesktop::WTS_CURRENT_SESSION,
                    name_buf.as_mut_ptr(),
                ),
                Syms::Cdecl { open, .. } => (open)(
                    ws::Win32::System::RemoteDesktop::WTS_CURRENT_SERVER_HANDLE,
                    ws::Win32::System::RemoteDesktop::WTS_CURRENT_SESSION,
                    name_buf.as_mut_ptr(),
                ),
            }
        };

        // Logged at debug level on purpose: when the underlying library crashes
        // the process, this is the only way to tell how far the open call got.
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
            name: display_name,
            wtshandle,
            name_buf,
            syms: self.syms.clone(),
        })
    }
}

pub struct Handle<'a> {
    name: Option<String>,
    wtshandle: ws::Win32::Foundation::HANDLE,
    // The channel name is handed over to the underlying library as a *mut
    // buffer, and VMware's implementation keeps that pointer around. The buffer
    // must therefore outlive the handle.
    #[allow(dead_code)]
    name_buf: Vec<i8>,
    syms: Syms<'a>,
}

// Because of the *mut content (handle) Rust does not derive Send and
// Sync. Since we know how those data will be used (especially in
// terms of concurrency) we assume to unsafely implement Send and
// Sync.
unsafe impl Send for Handle<'_> {}
unsafe impl Sync for Handle<'_> {}

impl vc::Handle for Handle<'_> {
    fn display_name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    fn read(&self, data: &mut [u8]) -> Result<ops::Range<usize>, vc::Error> {
        let to_read = os::raw::c_ulong::try_from(data.len())
            .map_err(|e| vc::Error::ReadFailed(e.to_string()))?;

        let timeout = os::raw::c_ulong::MAX;

        let mut read = 0;

        let ret = unsafe {
            match &self.syms {
                Syms::Stdcall { read: read_fn, .. } => (read_fn)(
                    self.wtshandle,
                    timeout,
                    data.as_mut_ptr(),
                    to_read,
                    &raw mut read,
                ),
                Syms::Cdecl { read: read_fn, .. } => (read_fn)(
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
                match &self.syms {
                    Syms::Stdcall { write, .. } => {
                        (write)(self.wtshandle, data.as_ptr(), to_write, &raw mut written)
                    }
                    Syms::Cdecl { write, .. } => {
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
            match &self.syms {
                Syms::Stdcall { close, .. } => (close)(self.wtshandle),
                Syms::Cdecl { close, .. } => (close)(self.wtshandle),
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
