use common::api;
use std::{fmt, sync};

mod citrix;
mod rdp;
mod semaphore;

#[derive(Clone, Debug)]
pub enum State {
    Initialized,
    Connected(Option<String>),
    Disconnected,
    Terminated,
}

pub enum Command {
    Open,
    SendChunk(api::Chunk),
    Close,
}

pub enum Response {
    ChangeState(State),
    ReceivedData(Vec<u8>),
    WriteCancelled,
}

pub enum Error {
    Citrix(citrix::Error),
    Rdp(rdp::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match self {
            Self::Citrix(e) => write!(f, "Citrix error: {e}"),
            Self::Rdp(e) => write!(f, "RDP error: {e}"),
        }
    }
}

trait SvcImplementation {
    fn open(&mut self) -> Result<(), Error>;
    fn write(&self, data: Vec<u8>) -> Result<(), Error>;
    fn close(&mut self) -> Result<(), Error>;
}

pub enum Svc {
    Citrix(citrix::Svc),
    Rdp(rdp::Svc),
}

impl Svc {
    pub fn open(&mut self) -> Result<(), Error> {
        match self {
            Self::Citrix(svc) => svc.open(),
            Self::Rdp(svc) => svc.open(),
        }
    }

    pub fn write(&self, data: Vec<u8>) -> Result<(), Error> {
        match self {
            Self::Citrix(svc) => svc.write(data),
            Self::Rdp(svc) => svc.write(data),
        }
    }

    pub fn close(&mut self) -> Result<(), Error> {
        match self {
            Self::Citrix(svc) => svc.close(),
            Self::Rdp(svc) => svc.close(),
        }
    }
}

pub static SVC: sync::RwLock<Option<Svc>> = sync::RwLock::new(None);

const MAX_CHUNKS_IN_FLIGHT: usize = 64;
