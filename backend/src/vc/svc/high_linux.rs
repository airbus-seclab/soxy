use crate::vc;
use std::{ffi, io, ops, os, ptr, thread, time};
use windows_sys as ws;

type SessionId = os::raw::c_uint;
const LF_CURRENT_SESSION: SessionId = 0xFFFF_FFFF;

type VcHandle = *const os::raw::c_void;

type CtxStatus = os::raw::c_uint;
const CTXSTAT_SUCCESS: CtxStatus = 0;

const fn convert_status(status: os::raw::c_uint) -> CtxStatus {
    match status {
        0 => 0,
        0x6d => 12,
        0x66 => 11,
        0x6c => 10,
        0x2d => 9,
        0x20 => 8,
        0x98 => 7,
        0x55 => 6,
        7 => 5,
        0xdb => 4,
        0x53 => 3,
        0x46 => 2,
        _ => 1,
    }
}

fn ctx_status_error_string(status: CtxStatus) -> String {
    match status {
        CTXSTAT_SUCCESS => "success".into(),
        1 => "failure".into(),
        2 => "invalid parameter".into(),
        3 => "not enough memory".into(),
        4 => "data error".into(),
        5 => "not supported".into(),
        6 => "WinSta no session".into(),
        7 => "channel not bound".into(),
        8 => "can't open".into(),
        9 => "timeout".into(),
        10 => "VC disabled".into(),
        11 => "VC disconnected".into(),
        12 => "VC already in use".into(),
        _ => format!("unknown CTXSTA {status}"),
    }
}

type WsVirtualOpenEx = unsafe extern "system" fn(
    null: *const os::raw::c_void,
    sessionid: SessionId,
    pvirtualname: *const os::raw::c_char,
    handle: *mut VcHandle,
    flags: os::raw::c_uint,
) -> os::raw::c_uint;

#[allow(non_snake_case)]
fn LfVirtualChannelOpen(
    ws_open_ex: &WsVirtualOpenEx,
    sessionid: SessionId,
    pvirtualname: *const os::raw::c_char,
    handle: *mut VcHandle,
) -> CtxStatus {
    convert_status(unsafe { (ws_open_ex)(ptr::null(), sessionid, pvirtualname, handle, 0) })
}

type WsVirtualRead = unsafe extern "system" fn(
    handle: VcHandle,
    pbuffer: *mut os::raw::c_uchar,
    length: os::raw::c_ulong,
    pbytesread: *mut os::raw::c_ulong,
) -> os::raw::c_uint;

#[allow(non_snake_case)]
fn LfVirtualChannelRead(
    ws_read: &WsVirtualRead,
    handle: VcHandle,
    pbuffer: *mut os::raw::c_uchar,
    length: os::raw::c_ulong,
    pbytesread: *mut os::raw::c_ulong,
) -> CtxStatus {
    convert_status(unsafe { (ws_read)(handle, pbuffer, length, pbytesread) })
}

type WsVirtualWrite = unsafe extern "system" fn(
    handle: VcHandle,
    pbuffer: *const os::raw::c_uchar,
    length: os::raw::c_ulong,
    pbyteswritten: *mut os::raw::c_ulong,
) -> os::raw::c_uint;

#[allow(non_snake_case)]
fn LfVirtualChannelWrite(
    ws_write: &WsVirtualWrite,
    handle: VcHandle,
    pbuffer: *const os::raw::c_uchar,
    length: os::raw::c_ulong,
    pbyteswritten: *mut os::raw::c_ulong,
) -> CtxStatus {
    convert_status(unsafe { (ws_write)(handle, pbuffer, length, pbyteswritten) })
}

type WsVirtualClose = unsafe extern "system" fn(handle: VcHandle) -> os::raw::c_uint;

#[allow(non_snake_case)]
fn LfVirtualChannelClose(ws_close: &WsVirtualClose, handle: VcHandle) -> CtxStatus {
    convert_status(unsafe { (ws_close)(handle) })
}

type EvtHandle = *const os::raw::c_uint;

type WsOpenConnection = unsafe extern "system" fn(handle: *mut EvtHandle) -> os::raw::c_uint;

#[allow(non_snake_case)]
fn LfSessionEventInit(ws_open_connection: &WsOpenConnection, handle: *mut EvtHandle) -> CtxStatus {
    convert_status(unsafe { (ws_open_connection)(handle) })
}

type WsCloseConnection = unsafe extern "system" fn(handle: *mut EvtHandle) -> os::raw::c_uint;

#[allow(non_snake_case)]
fn LfSessionEventDestroy(
    ws_close_connection: &WsCloseConnection,
    handle: *mut EvtHandle,
) -> CtxStatus {
    convert_status(unsafe { (ws_close_connection)(handle) })
}

pub enum Svc<'a> {
    Citrix {
        open_ex: libloading::Symbol<'a, WsVirtualOpenEx>,
        read: libloading::Symbol<'a, WsVirtualRead>,
        write: libloading::Symbol<'a, WsVirtualWrite>,
        close: libloading::Symbol<'a, WsVirtualClose>,
        event_open_connection: libloading::Symbol<'a, WsOpenConnection>,
        event_close_connection: libloading::Symbol<'a, WsCloseConnection>,
    },
    Standard {
        open: libloading::Symbol<'a, super::VirtualChannelOpen>,
        query: libloading::Symbol<'a, vc::VirtualChannelQuery>,
        read: libloading::Symbol<'a, vc::VirtualChannelRead>,
        write: libloading::Symbol<'a, vc::VirtualChannelWrite>,
        close: libloading::Symbol<'a, vc::VirtualChannelClose>,
    },
}

impl<'a> vc::VirtualChannel<'a> for Svc<'a> {
    type Handle = Handle<'a>;

    fn load(libs: &'a vc::Libraries) -> Result<Self, vc::Error> {
        if let Some(citrix) = libs.citrix() {
            unsafe {
                Ok(Self::Citrix {
                    open_ex: citrix.get(b"WsVirtualOpenEx")?,
                    read: citrix.get(b"WsVirtualRead")?,
                    write: citrix.get(b"WsVirtualWrite")?,
                    close: citrix.get(b"WsVirtualClose")?,
                    event_open_connection: citrix.get(b"WsOpenConnection")?,
                    event_close_connection: citrix.get(b"WsCloseConnection")?,
                })
            }
        } else if let Some(xrdp) = libs.xrdp() {
            #[cfg(feature = "log")]
            {
                common::debug!("initiate XRDP logging");

                let log_init = unsafe {
                    xrdp.get::<fn(os::raw::c_int, *mut os::raw::c_void) -> *mut os::raw::c_void>(
                        b"log_config_init_for_console",
                    )?
                };
                let log_start =
                    unsafe { xrdp.get::<fn(*mut os::raw::c_void)>(b"log_start_from_param")? };

                let lc = log_init(4, ptr::null_mut());

                if !lc.is_null() {
                    log_start(lc);
                }
            }

            unsafe {
                Ok(Self::Standard {
                    open: xrdp.get(b"WTSVirtualChannelOpen")?,
                    query: xrdp.get(b"WTSVirtualChannelQuery")?,
                    read: xrdp.get(b"WTSVirtualChannelRead")?,
                    write: xrdp.get(b"WTSVirtualChannelWrite")?,
                    close: xrdp.get(b"WTSVirtualChannelClose")?,
                })
            }
        } else {
            Err(vc::Error::NoLibraryFound)
        }
    }

    fn open(&self, name: [ffi::c_char; 8]) -> Result<Self::Handle, vc::Error> {
        common::debug!("trying to open SVC(high)");

        match self {
            Self::Citrix {
                open_ex,
                read,
                write,
                close,
                event_open_connection,
                event_close_connection,
            } => {
                let mut handle = ptr::null();

                let ret = LfVirtualChannelOpen(
                    open_ex,
                    LF_CURRENT_SESSION,
                    name.as_ptr(),
                    &raw mut handle,
                );

                if ret != CTXSTAT_SUCCESS {
                    return Err(vc::Error::OpenChannelFailed(ctx_status_error_string(ret)));
                }

                if handle.is_null() {
                    return Err(vc::Error::OpenChannelFailed("handle is NULL!".into()));
                }

                let mut event_handle = ptr::null();

                let ret = LfSessionEventInit(event_open_connection, &raw mut event_handle);

                if ret != CTXSTAT_SUCCESS {
                    return Err(vc::Error::OpenChannelFailed(ctx_status_error_string(ret)));
                }

                #[cfg(not(feature = "log"))]
                let name = None;

                #[cfg(feature = "log")]
                let name = Some(format!("SVC(High(Citrix)) {:?}", unsafe {
                    ffi::CStr::from_ptr(name.as_ptr())
                }));

                Ok(Handle::Citrix {
                    name,
                    handle,
                    read: read.clone(),
                    write: write.clone(),
                    close: close.clone(),
                    event_handle,
                    event_close_connection: event_close_connection.clone(),
                })
            }
            Self::Standard {
                open,
                query,
                read,
                write,
                close,
            } => {
                let handle = unsafe {
                    (open)(
                        ws::Win32::System::RemoteDesktop::WTS_CURRENT_SERVER_HANDLE,
                        ws::Win32::System::RemoteDesktop::WTS_CURRENT_SESSION,
                        name.as_ptr().cast_mut(),
                    )
                };

                if handle.is_null() {
                    let err = io::Error::last_os_error();
                    return Err(vc::Error::OpenChannelFailed(err.to_string()));
                }

                let mut client_dataptr = ptr::null_mut();
                let mut len = 0;

                common::trace!("VirtualChannelQuery");
                let ret = unsafe {
                    (query)(
                        handle,
                        ws::Win32::System::RemoteDesktop::WTSVirtualClientData,
                        ptr::from_mut(&mut client_dataptr),
                        &raw mut len,
                    )
                };

                if ret == ws::Win32::Foundation::FALSE {
                    let err = io::Error::last_os_error();
                    common::warn!("virtual channel query failed (len = {len}, last error = {err})");
                }

                #[cfg(not(feature = "log"))]
                let name = None;

                #[cfg(feature = "log")]
                let name = Some(format!("SVC(High(Standard)) {:?}", unsafe {
                    ffi::CStr::from_ptr(name.as_ptr())
                }));

                Ok(Handle::Standard {
                    name,
                    handle,
                    read: read.clone(),
                    write: write.clone(),
                    close: close.clone(),
                })
            }
        }
    }
}

pub enum Handle<'a> {
    Citrix {
        name: Option<String>,
        handle: VcHandle,
        read: libloading::Symbol<'a, WsVirtualRead>,
        write: libloading::Symbol<'a, WsVirtualWrite>,
        close: libloading::Symbol<'a, WsVirtualClose>,
        event_handle: EvtHandle,
        event_close_connection: libloading::Symbol<'a, WsCloseConnection>,
    },
    Standard {
        name: Option<String>,
        handle: ws::Win32::Foundation::HANDLE,
        read: libloading::Symbol<'a, vc::VirtualChannelRead>,
        write: libloading::Symbol<'a, vc::VirtualChannelWrite>,
        close: libloading::Symbol<'a, vc::VirtualChannelClose>,
    },
}

// Because of the *mut content (handle) Rust does not derive Send and
// Sync. Since we know how those data will be used (especially in
// terms of concurrency) we assume to unsafely implement Send and
// Sync.
unsafe impl Send for Handle<'_> {}
unsafe impl Sync for Handle<'_> {}

impl vc::Handle for Handle<'_> {
    fn display_name(&self) -> Option<&str> {
        match self {
            Self::Citrix { name, .. } | Self::Standard { name, .. } => {
                name.as_ref().map(String::as_str)
            }
        }
    }

    fn read(&self, data: &mut [u8]) -> Result<ops::Range<usize>, vc::Error> {
        let to_read = os::raw::c_ulong::try_from(data.len())
            .map_err(|e| vc::Error::ReadFailed(e.to_string()))?;

        let mut bread = 0;

        match self {
            Self::Citrix { handle, read, .. } => {
                let ret =
                    LfVirtualChannelRead(read, *handle, data.as_mut_ptr(), to_read, &raw mut bread);

                if ret != CTXSTAT_SUCCESS {
                    return Err(vc::Error::ReadFailed(ctx_status_error_string(ret)));
                }
            }
            Self::Standard { handle, read, .. } => loop {
                let timeout = 5000;

                let ret =
                    unsafe { (read)(*handle, timeout, data.as_mut_ptr(), to_read, &raw mut bread) };

                common::trace!(
                    "read returned ret == {ret} (== {}) bread = {bread}",
                    ws::Win32::Foundation::FALSE
                );

                if ret == ws::Win32::Foundation::FALSE {
                    let err = io::Error::last_os_error();
                    return Err(vc::Error::ReadFailed(err.to_string()));
                }

                if bread > 0 {
                    break;
                }

                common::trace!("read 0 byte without error, sleeping a bit");
                thread::sleep(time::Duration::from_millis(200));
            },
        }

        let read = usize::try_from(bread).map_err(|e| vc::Error::ReadFailed(e.to_string()))?;

        Ok(0..read)
    }

    fn write(&self, data: &[u8]) -> Result<usize, vc::Error> {
        let to_write = os::raw::c_ulong::try_from(data.len())
            .map_err(|e| vc::Error::WriteFailed(e.to_string()))?;

        let mut written = 0;

        common::trace!("write {to_write} bytes");

        match self {
            Self::Citrix { handle, write, .. } => {
                let ret = LfVirtualChannelWrite(
                    write,
                    *handle,
                    data.as_ptr(),
                    to_write,
                    &raw mut written,
                );

                if ret != CTXSTAT_SUCCESS || written != to_write {
                    if written != to_write {
                        common::error!("partial write: {written} / {to_write}");
                    }
                    return Err(vc::Error::WriteFailed(ctx_status_error_string(ret)));
                }
            }
            Self::Standard { handle, write, .. } => loop {
                let ret = unsafe { (write)(*handle, data.as_ptr(), to_write, &raw mut written) };

                if ret == ws::Win32::Foundation::FALSE || written != to_write {
                    common::trace!(
                        "write returned ret == {ret} (== {}?) written == {written} to_write == {to_write}",
                        ws::Win32::Foundation::FALSE
                    );

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

                break;
            },
        }

        usize::try_from(written).map_err(|e| vc::Error::WriteFailed(e.to_string()))
    }

    fn close(self) -> Result<(), vc::Error> {
        match self {
            Self::Citrix {
                handle,
                close,
                mut event_handle,
                event_close_connection,
                ..
            } => {
                let ret = LfSessionEventDestroy(&event_close_connection, &raw mut event_handle);
                if ret != CTXSTAT_SUCCESS {
                    common::debug!("failed to destroy event_handle");
                }

                let ret = LfVirtualChannelClose(&close, handle);
                if ret == CTXSTAT_SUCCESS {
                    Ok(())
                } else {
                    Err(vc::Error::CloseChannelFailed(ctx_status_error_string(ret)))
                }
            }
            Self::Standard { handle, close, .. } => {
                let ret = unsafe { (close)(handle) };
                if ret == ws::Win32::Foundation::FALSE {
                    let err = io::Error::last_os_error();
                    Err(vc::Error::CloseChannelFailed(err.to_string()))
                } else {
                    Ok(())
                }
            }
        }
    }
}
