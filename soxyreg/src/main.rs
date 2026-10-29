use clap::Parser;
use std::{env, path};

mod dvc;
mod horizon;
mod svc;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(value_enum)]
    mode: Mode,
    #[command(subcommand)]
    operation: Operation,
}

#[derive(clap::ValueEnum, Clone)]
enum Mode {
    Dvc,
    Horizon,
    Svc,
}

#[derive(clap::Subcommand)]
enum Operation {
    Register {
        dll: String,
    },
    /// The DLL path is only needed (and only used) by the `horizon` mode: it
    /// tells which entry to remove from the Horizon Client
    /// `VirtualChannelDLLs` list.
    Unregister {
        dll: Option<String>,
    },
}

/// Validates the DLL path given on the command line and makes it absolute, as
/// the registry consumers expect.
fn absolute_dll_path(dll: &str) -> Result<String, String> {
    let path = path::Path::new(dll);

    if !path.exists() {
        return Err("invalid DLL path".to_string());
    }

    if !path.is_file() {
        return Err("given path is not a file".to_string());
    }

    let path = if path.is_absolute() {
        path::PathBuf::from(path)
    } else {
        let curdir = env::current_dir().map_err(|e| format!("invalid current dir: {e}"))?;
        curdir.join(path)
    };

    path.as_os_str()
        .to_str()
        .map(str::to_string)
        .ok_or_else(|| "DLL path is not valid UTF-8".to_string())
}

fn main() {
    let args = Args::parse();

    match args.operation {
        Operation::Register { dll } => {
            let dll = match absolute_dll_path(&dll) {
                Ok(dll) => dll,
                Err(e) => {
                    eprintln!("{e}");
                    return;
                }
            };

            match args.mode {
                Mode::Dvc => dvc::register(&dll),
                Mode::Horizon => horizon::register(&dll),
                Mode::Svc => {
                    let file_name = path::Path::new(&dll)
                        .file_name()
                        .and_then(std::ffi::OsStr::to_str)
                        .unwrap_or("soxy.dll");

                    println!(
                        "Do not forget to put {file_name} in C:\\Program Files (x86)\\Citrix\\ICA Client\\ !!!!!!"
                    );

                    svc::register(&dll, file_name);
                }
            }
        }
        Operation::Unregister { dll } => match args.mode {
            Mode::Dvc => dvc::unregister(),
            Mode::Horizon => {
                let Some(dll) = dll else {
                    eprintln!(
                        "missing DLL path: it tells which entry to remove from the Horizon Client VirtualChannelDLLs list"
                    );
                    return;
                };
                horizon::unregister(&dll);
            }
            Mode::Svc => svc::unregister(),
        },
    }
}
