use crate::vc;
use std::{ffi, ops};

#[cfg(not(target_os = "windows"))]
mod high_linux;
#[cfg(target_os = "windows")]
mod high_windows;
#[cfg(target_os = "windows")]
mod low;

#[cfg(not(target_os = "windows"))]
use high_linux as high;
#[cfg(target_os = "windows")]
use high_windows as high;

pub enum Svc<'a> {
    High(high::Svc<'a>),
    #[cfg(target_os = "windows")]
    Low(low::Svc<'a>),
}

impl<'a> vc::VirtualChannel<'a> for Svc<'a> {
    type Handle = Handle<'a>;

    fn load(libs: &'a vc::Libraries) -> Result<Self, vc::Error> {
        // The "high" implementations (Citrix `WFVirtualChannel*`, Horizon
        // `VDP_VirtualChannel*`) are preferred over the plain WTS one, but the
        // Horizon one is not always usable: on VMware Horizon Agent,
        // `VDP_VirtualChannelOpen` returns a handle and then corrupts the caller
        // stack, so the process dies with an access violation once `open()` has
        // returned. `SOXY_SVC_LOW` (set to anything non-empty) skips the high
        // implementations and uses `WTSVirtualChannelOpen` instead.
        let force_low = std::env::var_os("SOXY_SVC_LOW").is_some_and(|v| !v.is_empty());

        let res = if force_low {
            common::debug!("SOXY_SVC_LOW is set, skipping the high implementations");
            Err(vc::Error::NoLibraryFound)
        } else {
            high::Svc::load(libs).map(Self::High)
        };
        #[cfg(target_os = "windows")]
        let res = res.or(low::Svc::load(libs).map(Self::Low));
        res
    }

    fn open(&self, name: [ffi::c_char; 8]) -> Result<Self::Handle, vc::Error> {
        match self {
            Self::High(svc) => svc.open(name).map(Handle::High),
            #[cfg(target_os = "windows")]
            Self::Low(svc) => svc.open(name).map(Handle::Low),
        }
    }
}

pub enum Handle<'a> {
    High(high::Handle<'a>),
    #[cfg(target_os = "windows")]
    Low(low::Handle<'a>),
}

impl vc::Handle for Handle<'_> {
    fn display_name(&self) -> Option<&str> {
        match self {
            Self::High(handle) => handle.display_name(),
            #[cfg(target_os = "windows")]
            Self::Low(handle) => handle.display_name(),
        }
    }

    fn read(&self, data: &mut [u8]) -> Result<ops::Range<usize>, vc::Error> {
        match self {
            Self::High(handle) => handle.read(data),
            #[cfg(target_os = "windows")]
            Self::Low(handle) => handle.read(data),
        }
    }

    fn write(&self, data: &[u8]) -> Result<usize, vc::Error> {
        match self {
            Self::High(handle) => handle.write(data),
            #[cfg(target_os = "windows")]
            Self::Low(handle) => handle.write(data),
        }
    }

    fn close(self) -> Result<(), vc::Error> {
        match self {
            Self::High(handle) => handle.close(),
            #[cfg(target_os = "windows")]
            Self::Low(handle) => handle.close(),
        }
    }
}
