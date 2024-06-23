//! tricore-gdb client
use gdbstub::conn::ConnectionExt;
use gdbstub::stub::state_machine::GdbStubStateMachine;
use gdbstub::stub::{DisconnectReason, GdbStub, MultiThreadStopReason};
use std::net::{TcpListener, TcpStream};
use std::num::NonZeroUsize;
use std::path::PathBuf;

use clap::{crate_version, value_parser};
use clap::{Arg, Command};

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
                .value_parser(value_parser!(PathBuf)),
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
            GdbStubStateMachine::Running(mut gdb) => {
                let stop_reason = target.clone().get_core_state();

                let conn = gdb.borrow_conn();
                let data_to_read = conn.peek().unwrap().is_some();

                if data_to_read {
                    let byte = gdb.borrow_conn().read().unwrap();
                    gdb.incoming_data(&mut target, byte).unwrap()
                } else if stop_reason.is_ok() {
                    gdb.report_stop(
                        &mut target,
                        MultiThreadStopReason::SwBreak(NonZeroUsize::new(1).unwrap()),
                    )
                    .unwrap()
                } else {
                    gdb.into()
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
