use super::protocol;
use crate::{rdp, service};
use std::{
    env, fs,
    io::{self, Write},
    path,
};

#[allow(clippy::too_many_lines)]
fn control_handler(mut stream: rdp::RdpStream<'_>) -> Result<(), io::Error> {
    crate::debug!("starting control");

    let mut cwd = env::current_dir()?;

    let mut quit = false;

    loop {
        let command = protocol::ControlCommand::receive(&mut stream)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

        crate::trace!("received {command:?}");

        let resp = match command {
            protocol::ControlCommand::Cdup => {
                if cwd.pop() {
                    protocol::ControlResponse::Ok(250, None)
                } else {
                    protocol::ControlResponse::Error(550)
                }
            }
            protocol::ControlCommand::Cwd(path) => {
                let path = if path.starts_with('/') {
                    path::PathBuf::from(path)
                } else {
                    cwd.join(path)
                };
                if path.exists() && path.is_dir() {
                    cwd = path;
                    protocol::ControlResponse::Ok(250, None)
                } else {
                    protocol::ControlResponse::Error(550)
                }
            }
            protocol::ControlCommand::Dele(path) => {
                let path = if path.starts_with('/') {
                    path::PathBuf::from(path)
                } else {
                    cwd.join(path)
                };
                if let Err(e) = fs::remove_file(&path) {
                    crate::warn!("failed to delete {path:?}: {e}");
                    protocol::ControlResponse::Error(550)
                } else {
                    protocol::ControlResponse::Ok(200, None)
                }
            }
            protocol::ControlCommand::Epsv => protocol::ControlResponse::Epsv,
            protocol::ControlCommand::Feat => protocol::ControlResponse::Feat,
            protocol::ControlCommand::List => protocol::ControlResponse::Data(
                protocol::DataCommand::List(cwd.display().to_string()),
            ),
            protocol::ControlCommand::Nlst => protocol::ControlResponse::Data(
                protocol::DataCommand::Nlst(cwd.display().to_string()),
            ),
            protocol::ControlCommand::Opts => protocol::ControlResponse::Ok(200, None),
            protocol::ControlCommand::Pass | protocol::ControlCommand::Type => {
                protocol::ControlResponse::Ok(230, None)
            }
            protocol::ControlCommand::Pasv => protocol::ControlResponse::Pasv,
            protocol::ControlCommand::Pwd => {
                protocol::ControlResponse::Ok(257, Some(format!("{:?}", cwd.display())))
            }
            protocol::ControlCommand::Quit => {
                quit = true;
                protocol::ControlResponse::Quit
            }
            protocol::ControlCommand::Retr(path) => {
                let path = if path.starts_with('/') {
                    path::PathBuf::from(path)
                } else {
                    cwd.join(path)
                };
                if path.exists() && path.is_file() {
                    protocol::ControlResponse::Data(protocol::DataCommand::Retr(
                        path.display().to_string(),
                    ))
                } else {
                    protocol::ControlResponse::Error(550)
                }
            }
            protocol::ControlCommand::Size(path) => {
                let path = if path.starts_with('/') {
                    path::PathBuf::from(path)
                } else {
                    cwd.join(path)
                };
                path.metadata()
                    .map_or(protocol::ControlResponse::Error(540), |metadata| {
                        let size = metadata.len();
                        protocol::ControlResponse::Ok(213, Some(format!("{size}")))
                    })
            }
            protocol::ControlCommand::Stor(path) => {
                let path = if path.starts_with('/') {
                    path::PathBuf::from(path)
                } else {
                    cwd.join(path)
                };
                // RFC 959: STOR creates the file if it does not exist and
                // replaces it if it does. The data handler opens it with
                // truncate(true) so an existing file is fully overwritten.
                protocol::ControlResponse::Data(protocol::DataCommand::Stor(
                    path.display().to_string(),
                ))
            }
            protocol::ControlCommand::User => protocol::ControlResponse::Ok(331, None),
        };

        resp.send(&mut stream)
            .map_err(|e| io::Error::new(io::ErrorKind::BrokenPipe, e.to_string()))?;

        if quit {
            return Ok(());
        }
    }
}

fn data_handler(mut stream: rdp::RdpStream<'_>) -> Result<(), io::Error> {
    crate::debug!("starting data");

    let cmd = protocol::DataCommand::receive(&mut stream)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

    crate::debug!("received {cmd:?}");

    match cmd {
        protocol::DataCommand::List(path) => {
            let path = path::PathBuf::from(path);
            if let Ok(dir) = path.read_dir() {
                dir.into_iter().try_for_each(|entry| {
                    if let Ok(entry) = entry
                        && let Ok(file_type) = entry.file_type()
                    {
                        if file_type.is_dir() {
                            write!(stream, "d")?;
                        } else if file_type.is_file() {
                            write!(stream, "-")?;
                        } else {
                            write!(stream, "l")?;
                        }
                        let _ = write!(stream, "rwxrwxrwx ");

                        if file_type.is_dir() {
                            let _ = write!(stream, "2 ftp ftp ");
                        } else {
                            let _ = write!(stream, "1 ftp ftp ");
                        }

                        if let Ok(metadata) = entry.metadata() {
                            write!(stream, "{} ", metadata.len())?;
                        } else {
                            write!(stream, "0 ")?;
                        }

                        write!(stream, "Jan 1 1970 ")?;

                        write!(stream, "{}\r\n", entry.file_name().into_string().unwrap())?;
                    }

                    Ok::<(), io::Error>(())
                })?;
            }
        }

        protocol::DataCommand::Nlst(path) => {
            let path = path::PathBuf::from(path);
            if let Ok(dir) = path.read_dir() {
                dir.into_iter().try_for_each(|entry| {
                    if let Ok(entry) = entry {
                        write!(stream, "{}\r\n", entry.file_name().into_string().unwrap())?;
                    }
                    Ok::<(), io::Error>(())
                })?;
            }
        }

        protocol::DataCommand::Retr(path) => {
            let path = path::PathBuf::from(path);
            let file = fs::File::options().read(true).write(false).open(path)?;
            let mut file = io::BufReader::new(file);
            if let Err(e) = service::stream_copy(&mut file, &mut stream, false) {
                crate::debug!("error: {e}");
            }
        }

        protocol::DataCommand::Stor(path) => {
            let path = path::PathBuf::from(path);
            let file = fs::File::options()
                .create(true)
                .truncate(true)
                .write(true)
                .open(path)?;
            let mut file = io::BufWriter::new(file);
            if let Err(e) = service::stream_copy(&mut stream, &mut file, false) {
                crate::debug!("error: {e}");
            }
        }
    }

    stream.flush()?;

    Ok(())
}

pub fn handler(mut stream: rdp::RdpStream<'_>) -> Result<(), io::Error> {
    let mode = protocol::BackendMode::receive(&mut stream)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

    match mode {
        protocol::BackendMode::Control => control_handler(stream),
        protocol::BackendMode::Data => data_handler(stream),
    }
}
