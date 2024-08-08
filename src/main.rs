//! tricore-gdb client
use clap::{crate_version, value_parser};
use clap::{Arg, Command};
use gdb::{tricore, StaticTricoreTarget};
use gdbstub::common::Signal;
use gdbstub::conn::{Connection, ConnectionExt};
use gdbstub::stub::{run_blocking, DisconnectReason, GdbStub, SingleThreadStopReason};
use gdbstub::target::Target;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;

// pub mod backtrace;
mod gdb;
use crate::gdb::TricoreTarget;

type DynResult<T> = Result<T, Box<dyn std::error::Error>>;

fn wait_for_tcp(port: u16, tcp_ip: &String) -> DynResult<TcpStream> {
    let sockaddr = format!("{}:{}", tcp_ip, port);
    println!("Waiting for a GDB connection on {:?}...", sockaddr);

    let sock = TcpListener::bind(sockaddr)?;
    let (stream, addr) = sock.accept()?;
    println!("Debugger connected from {}", addr);

    stream.set_nodelay(true).expect("set_nodelay call failed");

    Ok(stream)
}

enum TricoreGdbEventLoop {}

// type StaticTricoreTarget = TricoreTarget<'static>;

impl run_blocking::BlockingEventLoop for TricoreGdbEventLoop {
    type Target = StaticTricoreTarget;
    type Connection = Box<dyn ConnectionExt<Error = std::io::Error>>;
    type StopReason = SingleThreadStopReason<u32>;

    #[allow(clippy::type_complexity)]
    fn wait_for_stop_reason(
        target: &mut StaticTricoreTarget,
        conn: &mut Self::Connection,
    ) -> Result<
        run_blocking::Event<SingleThreadStopReason<u32>>,
        run_blocking::WaitForStopReasonError<
            <Self::Target as Target>::Error,
            <Self::Connection as Connection>::Error,
        >,
    > {
        let poll_incoming_data = || {
            // gdbstub takes ownership of the underlying connection, so the `borrow_conn`
            // method is used to borrow the underlying connection back from the stub to
            // check for incoming data.
            conn.peek().map(|b| b.is_some()).unwrap_or(true)
        };

        match target.run(poll_incoming_data) {
            tricore::RunEvent::IncomingData => {
                let byte = conn
                    .read()
                    .map_err(run_blocking::WaitForStopReasonError::Connection)?;
                Ok(run_blocking::Event::IncomingData(byte))
            }
            tricore::RunEvent::Event(event) => {
                use gdbstub::target::ext::breakpoints::WatchKind;

                let stop_reason = match event {
                    tricore::Event::DoneStep => SingleThreadStopReason::DoneStep,
                    tricore::Event::Halted => SingleThreadStopReason::Terminated(Signal::SIGSTOP),
                    tricore::Event::Break => SingleThreadStopReason::SwBreak(()),
                    tricore::Event::WatchWrite(addr) => SingleThreadStopReason::Watch {
                        tid: (),
                        kind: WatchKind::Write,
                        addr,
                    },
                    tricore::Event::WatchRead(addr) => SingleThreadStopReason::Watch {
                        tid: (),
                        kind: WatchKind::Read,
                        addr,
                    },
                };

                Ok(run_blocking::Event::TargetStopped(stop_reason))
            }
        }
    }

    fn on_interrupt(
        _target: &mut TricoreTarget,
    ) -> Result<Option<SingleThreadStopReason<u32>>, <StaticTricoreTarget as Target>::Error> {
        Ok(Some(SingleThreadStopReason::Signal(Signal::SIGINT)))
    }
}

fn main() -> Result<(), i32> {
    pretty_env_logger::init();
    let about = "GDB client interface via miniwiggler".to_string();

    let matches = Command::new("tricore-gdb-das")
        .version(crate_version!()) // Get version from Cargo.toml
        .about(about)
        .arg(
            Arg::new("elf_file")
                .long("elf_file")
                .value_name("FILE")
                .required(false)
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("tcp_ip")
                .long("tcp_ip")
                .value_name("TCP_IP")
                .required(false)
                .value_parser(value_parser!(String))
                .default_value("127.0.0.1"),
        )
        .arg(
            Arg::new("tcp_port")
                .long("tcp_port")
                .value_name("TCP_PORT")
                .required(false)
                .value_parser(value_parser!(u16))
                .default_value("9001"),
        )
        .get_matches();

    let file_path = matches.get_one::<PathBuf>("elf_file");

    let mut target = match TricoreTarget::new(file_path) {
        Ok(target) => target,
        Err(_) => return Err(-1),
    };

    let connection: Box<dyn ConnectionExt<Error = std::io::Error>> = {
        let tcp_port = matches.get_one::<u16>("tcp_port").unwrap();
        let tcp_ip = matches.get_one::<String>("tcp_ip").unwrap();
        Box::new(match wait_for_tcp(*tcp_port, tcp_ip) {
            Ok(tc) => tc,
            Err(_) => return Err(-1),
        })
    };

    let gdb = GdbStub::new(connection);

    match gdb.run_blocking::<TricoreGdbEventLoop>(&mut target) {
        Ok(disconnect_reason) => match disconnect_reason {
            DisconnectReason::Disconnect => {
                println!("GDB client has disconnected. Running to completion...");
            }
            DisconnectReason::TargetExited(code) => {
                println!("Target exited with code {}!", code)
            }
            DisconnectReason::TargetTerminated(sig) => {
                println!("Target terminated with signal {}!", sig)
            }
            DisconnectReason::Kill => println!("GDB sent a kill command!"),
        },
        Err(e) => {
            if e.is_target_error() {
                println!(
                    "target encountered a fatal error: {}",
                    e.into_target_error().unwrap()
                )
            } else if e.is_connection_error() {
                let (e, kind) = e.into_connection_error().unwrap();
                println!("connection error: {:?} - {}", kind, e,)
            } else {
                println!("gdbstub encountered a fatal error: {}", e)
            }
        }
    }

    println!("Program completed");

    Ok(())
}
