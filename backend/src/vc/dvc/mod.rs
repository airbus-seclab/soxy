use crate::vc;
use std::{ffi, ops, os};
use windows_sys as ws;

#[cfg(target_os = "windows")]
mod wts;
mod xrdp;

type VirtualChannelOpenEx = unsafe extern "system" fn(
    sessionid: os::raw::c_uint,
    pvirtualname: *mut os::raw::c_char,
    flags: os::raw::c_uint,
) -> ws::Win32::Foundation::HANDLE;

pub enum Dvc<'a> {
    Xrdp(xrdp::Dvc<'a>),
    #[cfg(target_os = "windows")]
    Wts(wts::Dvc<'a>),
}

impl<'a> vc::VirtualChannel<'a> for Dvc<'a> {
    type Handle = Handle<'a>;

    fn load(libs: &'a vc::Libraries) -> Result<Self, vc::Error> {
        let res = xrdp::Dvc::load(libs).map(Self::Xrdp);
        #[cfg(target_os = "windows")]
        let res = res.or(wts::Dvc::load(libs).map(Self::Wts));
        res
    }

    fn open(&'a self, name: [ffi::c_char; 8]) -> Result<Self::Handle, vc::Error> {
        match self {
            Self::Xrdp(dvc) => dvc.open(name).map(Handle::Xrdp),
            #[cfg(target_os = "windows")]
            Self::Wts(dvc) => dvc.open(name).map(Handle::Wts),
        }
    }
}

pub enum Handle<'a> {
    Xrdp(xrdp::Handle<'a>),
    #[cfg(target_os = "windows")]
    Wts(wts::Handle<'a>),
}

impl vc::Handle for Handle<'_> {
    fn display_name(&self) -> Option<&str> {
        match self {
            Self::Xrdp(handle) => handle.display_name(),
            #[cfg(target_os = "windows")]
            Self::Wts(handle) => handle.display_name(),
        }
    }

    fn read(&self, data: &mut [u8]) -> Result<ops::Range<usize>, vc::Error> {
        match self {
            Self::Xrdp(handle) => handle.read(data),
            #[cfg(target_os = "windows")]
            Self::Wts(handle) => handle.read(data),
        }
    }

    fn write(&self, data: &[u8]) -> Result<usize, vc::Error> {
        match self {
            Self::Xrdp(handle) => handle.write(data),
            #[cfg(target_os = "windows")]
            Self::Wts(handle) => handle.write(data),
        }
    }

    fn close(self) -> Result<(), vc::Error> {
        match self {
            Self::Xrdp(handle) => handle.close(),
            #[cfg(target_os = "windows")]
            Self::Wts(handle) => handle.close(),
        }
    }
}
