use crate::vc;
use std::{cell, ffi, io, ops, os, ptr};
use windows_sys as ws;

pub struct Svc<'a> {
    open: libloading::Symbol<'a, super::VirtualChannelOpen>,
    query: libloading::Symbol<'a, vc::VirtualChannelQuery>,
    close: libloading::Symbol<'a, vc::VirtualChannelClose>,
}

impl<'a> vc::VirtualChannel<'a> for Svc<'a> {
    type Handle = Handle<'a>;

    fn load(libs: &'a vc::Libraries) -> Result<Self, vc::Error> {
        libs.wts()
            .ok_or(vc::Error::NoLibraryFound)
            .and_then(|wts| unsafe {
                Ok(Self {
                    open: wts.get(b"WTSVirtualChannelOpen")?,
                    query: wts.get(b"WTSVirtualChannelQuery")?,
                    close: wts.get(b"WTSVirtualChannelClose")?,
                })
            })
    }

    fn open(&self, name: [ffi::c_char; 8]) -> Result<Self::Handle, vc::Error> {
        common::debug!("trying to open SVC(low)");

        let wtshandle = unsafe {
            (self.open)(
                ws::Win32::System::RemoteDesktop::WTS_CURRENT_SERVER_HANDLE,
                ws::Win32::System::RemoteDesktop::WTS_CURRENT_SESSION,
                name.as_ptr().cast_mut(),
            )
        };

        if wtshandle.is_null() {
            let err = io::Error::last_os_error();
            return Err(vc::Error::OpenChannelFailed(err.to_string()));
        }

        let mut filehandleptr: *mut ws::Win32::Foundation::HANDLE = ptr::null_mut();
        let filehandleptrptr: *mut *mut ws::Win32::Foundation::HANDLE = &raw mut filehandleptr;
        let mut len = 0;

        common::trace!("VirtualChannelQuery");
        let ret = unsafe {
            (self.query)(
                wtshandle,
                ws::Win32::System::RemoteDesktop::WTSVirtualFileHandle,
                filehandleptrptr.cast(),
                &raw mut len,
            )
        };
        if ret == ws::Win32::Foundation::FALSE {
            let err = io::Error::last_os_error();
            return Err(vc::Error::QueryFailed(err.to_string()));
        }
        if filehandleptr.is_null() {
            let err = io::Error::last_os_error();
            return Err(vc::Error::QueryFailed(err.to_string()));
        }

        let filehandle = unsafe { filehandleptr.read() };

        common::trace!("filehandle = {filehandle:?}");

        let mut dfilehandle: ws::Win32::Foundation::HANDLE = ptr::null_mut();

        common::trace!("DuplicateHandle");
        let ret = unsafe {
            ws::Win32::Foundation::DuplicateHandle(
                ws::Win32::System::Threading::GetCurrentProcess(),
                filehandle,
                ws::Win32::System::Threading::GetCurrentProcess(),
                &raw mut dfilehandle,
                0,
                ws::Win32::Foundation::FALSE,
                ws::Win32::Foundation::DUPLICATE_SAME_ACCESS,
            )
        };
        if ret == ws::Win32::Foundation::FALSE {
            let err = io::Error::last_os_error();
            return Err(vc::Error::DuplicateHandleFailed(err.to_string()));
        }
        common::trace!("duplicated filehandle = {dfilehandle:?}");

        let read_overlapped = cell::RefCell::new(create_io_overlapped()?);
        let write_overlapped = cell::RefCell::new(create_io_overlapped()?);

        #[cfg(not(feature = "log"))]
        let name = None;

        #[cfg(feature = "log")]
        let name = Some(format!("SVC(low) {:?}", unsafe {
            ffi::CStr::from_ptr(name.as_ptr())
        }));

        Ok(Handle {
            name,
            channelhandle: wtshandle,
            close: self.close.clone(),
            filehandle: dfilehandle,
            read_overlapped,
            write_overlapped,
        })
    }
}

fn create_io_overlapped() -> Result<ws::Win32::System::IO::OVERLAPPED, vc::Error> {
    let h_event = unsafe {
        ws::Win32::System::Threading::CreateEventA(
            ptr::null(),
            ws::Win32::Foundation::FALSE,
            ws::Win32::Foundation::FALSE,
            ptr::null(),
        )
    };

    if h_event.is_null() {
        let err = io::Error::last_os_error();
        return Err(vc::Error::CreateEventFailed(err.to_string()));
    }

    let anonymous = ws::Win32::System::IO::OVERLAPPED_0 {
        Pointer: ptr::null_mut(),
    };

    Ok(ws::Win32::System::IO::OVERLAPPED {
        Internal: 0,
        InternalHigh: 0,
        Anonymous: anonymous,
        hEvent: h_event,
    })
}

pub struct Handle<'a> {
    name: Option<String>,
    channelhandle: ws::Win32::Foundation::HANDLE,
    close: libloading::Symbol<'a, vc::VirtualChannelClose>,
    filehandle: ws::Win32::Foundation::HANDLE,
    read_overlapped: cell::RefCell<ws::Win32::System::IO::OVERLAPPED>,
    write_overlapped: cell::RefCell<ws::Win32::System::IO::OVERLAPPED>,
}

// Because of the *mut content (handle but also in OVERLAPPED
// structure) Rust does not derive Send and Sync. Since we know how
// those data will be used (especially in terms of concurrency) we
// assume to unsafely implement Send and Sync.
unsafe impl Send for Handle<'_> {}
unsafe impl Sync for Handle<'_> {}

impl vc::Handle for Handle<'_> {
    fn display_name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    fn read(&self, data: &mut [u8]) -> Result<ops::Range<usize>, vc::Error> {
        let to_read = os::raw::c_ulong::try_from(data.len())
            .map_err(|e| vc::Error::ReadFailed(e.to_string()))?;

        let mut read = 0;

        let mut overlapped = self.read_overlapped.borrow_mut();

        let ret = unsafe {
            ws::Win32::Storage::FileSystem::ReadFile(
                self.filehandle,
                data.as_mut_ptr(),
                to_read,
                &raw mut read,
                &raw mut *overlapped,
            )
        };

        if ret == ws::Win32::Foundation::FALSE {
            let ret = unsafe { ws::Win32::Foundation::GetLastError() };
            if ret == ws::Win32::Foundation::ERROR_IO_PENDING {
                let mut read = 0;
                let ret = unsafe {
                    ws::Win32::System::IO::GetOverlappedResult(
                        self.filehandle,
                        &raw const *overlapped,
                        &raw mut read,
                        ws::Win32::Foundation::TRUE,
                    )
                };
                if ret == ws::Win32::Foundation::FALSE {
                    let err = io::Error::last_os_error();
                    Err(vc::Error::ReadFailed(err.to_string()))
                } else {
                    Ok(0..read as usize)
                }
            } else {
                #[cfg(not(feature = "log"))]
                let e = { Err(vc::Error::ReadFailed(String::new())) };

                #[cfg(feature = "log")]
                let e = {
                    let err = io::Error::last_os_error();
                    Err(vc::Error::ReadFailed(format!("ret == 0x{ret:x?} {err}")))
                };

                e
            }
        } else {
            Ok(0..read as usize)
        }
    }

    fn write(&self, data: &[u8]) -> Result<usize, vc::Error> {
        let to_write = os::raw::c_ulong::try_from(data.len())
            .map_err(|e| vc::Error::WriteFailed(e.to_string()))?;

        let mut written = 0;

        let mut overlapped = self.write_overlapped.borrow_mut();

        let ret = unsafe {
            ws::Win32::Storage::FileSystem::WriteFile(
                self.filehandle,
                data.as_ptr(),
                to_write,
                &raw mut written,
                &raw mut *overlapped,
            )
        };

        if ret == ws::Win32::Foundation::FALSE {
            let ret = unsafe { ws::Win32::Foundation::GetLastError() };
            if ret == ws::Win32::Foundation::ERROR_IO_PENDING {
                let mut written = 0;
                let ret = unsafe {
                    ws::Win32::System::IO::GetOverlappedResult(
                        self.filehandle,
                        &raw const *overlapped,
                        &raw mut written,
                        ws::Win32::Foundation::TRUE,
                    )
                };
                if ret == ws::Win32::Foundation::FALSE {
                    let err = io::Error::last_os_error();
                    Err(vc::Error::WriteFailed(err.to_string()))
                } else {
                    Ok(written as usize)
                }
            } else {
                let err = io::Error::last_os_error();
                Err(vc::Error::WriteFailed(format!("ret == 0x{ret:x?} {err}")))
            }
        } else {
            Ok(written as usize)
        }
    }

    fn close(self) -> Result<(), vc::Error> {
        let ret = unsafe { ws::Win32::Foundation::CloseHandle(self.filehandle) };
        if ret == ws::Win32::Foundation::FALSE {
            let err = io::Error::last_os_error();
            return Err(vc::Error::CloseChannelFailed(err.to_string()));
        }

        let ret = unsafe { (self.close)(self.channelhandle) };
        if ret == ws::Win32::Foundation::FALSE {
            let err = io::Error::last_os_error();
            return Err(vc::Error::CloseChannelFailed(err.to_string()));
        }

        Ok(())
    }
}
