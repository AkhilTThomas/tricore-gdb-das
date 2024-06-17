//! tricore-gdb client
use gdbstub::common::Signal;
use gdbstub::conn::{Connection, ConnectionExt};
use gdbstub::stub::{run_blocking, state_machine::GdbStubStateMachine};
use gdbstub::stub::{DisconnectReason, GdbStub, MultiThreadStopReason, SingleThreadStopReason};
use gdbstub::target::Target;

use std::net::{TcpListener, TcpStream};

use rust_mcd::core::CoreState;
use std::path::PathBuf;

use clap::{crate_version, value_parser};
use clap::{Arg, Command};
use gdbstub::stub::BaseStopReason::Exited;

// pub mod backtrace;
mod gdb;
use crate::gdb::TricoreTarget;

type DynResult<T> = Result<T, Box<dyn std::error::Error>>;

fn wait_for_tcp(port: u16) -> DynResult<TcpStream> {
    let sockaddr = format!("127.0.0.1:{}", port);
    eprintln!("Waiting for a GDB connection on {:?}...", sockaddr);

    let sock = TcpListener::bind(sockaddr)?;
    let (stream, addr) = sock.accept()?;
    eprintln!("Debugger connected from {}", addr);

    Ok(stream)
}
enum TricoreGdbEventLoop {}

impl run_blocking::BlockingEventLoop for TricoreGdbEventLoop {
    type Target = TricoreTarget;
    type Connection = Box<dyn ConnectionExt<Error = std::io::Error>>;
    type StopReason = SingleThreadStopReason<u32>;

    #[allow(clippy::type_complexity)]
    fn wait_for_stop_reason(
        target: &mut TricoreTarget,
        conn: &mut Self::Connection,
    ) -> Result<
        run_blocking::Event<SingleThreadStopReason<u32>>,
        run_blocking::WaitForStopReasonError<
            <Self::Target as Target>::Error,
            <Self::Connection as Connection>::Error,
        >,
    > {
        let core = match target.system.get_core(0) {
            Ok(core) => core,
            Err(_) => {
                return Err(run_blocking::WaitForStopReasonError::Target(
                    "Fatal error",
                ))
            }
        };
        loop {
            let state = match core.query_state() {
                Ok(state) => state,
                Err(_) => {
                    return Err(run_blocking::WaitForStopReasonError::Target(
                        "Fatal error",
                    ))
                }
            };
            if state.state != CoreState::Running {
                // println!("Core in state {:?}",state.state);
                break;
            }
        }

        Ok(run_blocking::Event::TargetStopped(Exited(0)))
    }

    fn on_interrupt(
        _target: &mut TricoreTarget,
    ) -> Result<Option<SingleThreadStopReason<u32>>, <TricoreTarget as Target>::Error> {
        // Because this emulator runs as part of the GDB stub loop, there isn't any
        // special action that needs to be taken to interrupt the underlying target. It
        // is implicitly paused whenever the stub isn't within the
        // `wait_for_stop_reason` callback.
        Ok(Some(SingleThreadStopReason::Signal(Signal::SIGINT)))
    }
}

fn main() -> Result<(), i32> {
    let about = "GDB client interface via miniwiggler".to_string();

    let matches = Command::new("tricore-gdb-das")
        .version(crate_version!()) // Get version from Cargo.toml
        .about(about)
        .arg(
            Arg::new("elf_file")
                .long("elf_file")
                .value_name("FILE")
                .required(false)
                .value_parser(value_parser!(PathBuf))
        )
        .arg(
            Arg::new("tcp_port")
                .long("tcp_port")
                .value_name("TCP_PORT")
                .required(false)
                .value_parser(value_parser!(u16))
                .default_value("9001")
        )
        .get_matches();

    let file_path = matches.get_one::<PathBuf>("elf_file");

    let mut target = match TricoreTarget::new(file_path) {
        Ok(target) => target,
        Err(_) => return Err(-1),
    };

    let connection: Box<dyn ConnectionExt<Error = std::io::Error>> = {
        let tcp_port = matches.get_one::<u16>("tcp_port").unwrap();
        Box::new(match wait_for_tcp(*tcp_port) {
            Ok(tc) => tc,
            Err(_) => return Err(-1),
        })
    };

    let gdb = GdbStub::new(connection);

    let mut gdb = match gdb.run_state_machine(&mut target) {
        Ok(gdb) => gdb,
        Err(e) => return Err(-1),
    };

    let res = loop {
        gdb = match gdb {
            GdbStubStateMachine::Idle(mut gdb) => {
                let byte = gdb.borrow_conn().read().unwrap();
                gdb.incoming_data(&mut target, byte).unwrap()
            }
            GdbStubStateMachine::Running(gdb) => {
                match gdb.report_stop(&mut target, MultiThreadStopReason::DoneStep) {
                    Ok(gdb) => gdb,
                    Err(e) => {
                        break {
                            println!("running err");
                            Err(e)
                        }
                    }
                }
            }
            GdbStubStateMachine::CtrlCInterrupt(gdb) => {
                println!("ctrC trigg");
                match gdb.interrupt_handled(&mut target, None::<MultiThreadStopReason<u32>>) {
                    Ok(gdb) => gdb,
                    Err(e) => {
                        println!("ctrC err");
                        break Err(e);
                    }
                }
            }
            GdbStubStateMachine::Disconnected(gdb) => break Ok(gdb.get_reason()),
        }
    };

    match res {
        Ok(disconnect_reason) => match disconnect_reason {
            DisconnectReason::Disconnect => println!("GDB Disconnected"),
            DisconnectReason::TargetExited(_) => println!("Target exited"),
            DisconnectReason::TargetTerminated(_) => println!("Target halted"),
            DisconnectReason::Kill => println!("GDB sent a kill command"),
        },
        Err(e) => {
            if e.is_target_error() {
                println!("Target raised a fatal error");
            } else {
                print!("gdbstub internal error");
            }
        }
    }

    println!("Program completed");

    Ok(())
}
