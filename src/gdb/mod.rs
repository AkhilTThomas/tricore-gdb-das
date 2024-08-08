use crate::DynResult;
use anyhow::{Context, Result};
use gdbstub::common::Signal;
use gdbstub::target;
use gdbstub::target::ext::base::singlethread::{
    SingleThreadBase, SingleThreadResume, SingleThreadResumeOps,
};
use gdbstub::target::ext::breakpoints::{Breakpoints, BreakpointsOps, SwBreakpointOps};
use gdbstub::target::TargetResult;
use gdbstub::target::{Target, TargetError};
use gdbstub_arch::tricore::TricoreV1_6;
use rust_mcd::core::{CoreState, Trigger};
use rust_mcd::reset::ResetClass;
use gdbstub::target::ext::monitor_cmd::ConsoleOutput;
mod chip_communication;
use gdbstub::target::ext::monitor_cmd::outputln;
pub mod das;
pub mod elf;
pub mod flash;
pub mod tricore;
use gdbstub::conn::{Connection, ConnectionExt};
use chip_communication::DeviceSelection;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;

fn pretty_print_devices(devices: &[DeviceSelection]) {
    if devices.is_empty() {
        println!("No devices available");
        return;
    }
    println!("Found {} devices:", devices.len());
    for (index, scanned_device) in devices.iter().enumerate() {
        println!("Device {index}: {:?}", scanned_device.info.acc_hw())
    }
}

pub struct TricoreTarget<'a> {
    pub(crate) breakpoints: HashMap<u32, Trigger<'a>>,
    pub(crate) system: rust_mcd::system::System,
    pub(crate) core: rust_mcd::core::Core<'a>,
}

pub type StaticTricoreTarget = TricoreTarget<'static>;

impl TricoreTarget<'static> {
    pub fn new(program_elf: Option<&PathBuf>) -> DynResult<TricoreTarget<'static>> {
        let mut command_server = chip_communication::ChipCommunication::new()?;
        let scanned_devices = command_server.list_devices()?;

        if scanned_devices.is_empty() {
            return Err("No devices found".into());
        }

        pretty_print_devices(&scanned_devices);

        command_server.connect(Some(&scanned_devices[0]))?;

        match program_elf {
            Some(program_elf) => {
                println!("Programming via elf: {:?}", program_elf);
                command_server
                    .flash_elf(program_elf)
                    .context("Cannot flash elf")?;

                println!("Sucessfully flashed {:?} ", program_elf);
            }
            None => println!("No elf provided..."),
        }

        let system = command_server.get_system()?;
        let core = system.get_core(0)?;
        let system_reset = ResetClass::construct_reset_class(&core, 0);
        core.reset(system_reset, true)?;

        let static_core = unsafe {
            std::mem::transmute::<rust_mcd::core::Core<'_>, rust_mcd::core::Core<'static>>(core)
        };

        sleep(Duration::from_secs(2));

        // Do we also need to reset the other cores?

        Ok(TricoreTarget {
            breakpoints: HashMap::new(),
            system,
            core: static_core,
        })
    }


    // run till event
    pub fn run(&mut self, mut poll_incoming_data: impl FnMut() -> bool) -> tricore::RunEvent {
        loop {
            // poll for incoming data
            if poll_incoming_data() {
                break tricore::RunEvent::IncomingData;
            }

            let core_info = self.core.query_state();

            match core_info {
                Ok(core_info) => match core_info.state {
                    CoreState::Custom => todo!(),
                    CoreState::Debug => return tricore::RunEvent::Event(tricore::Event::Break),
                    CoreState::Halted => todo!(),
                    CoreState::Running => {}
                    CoreState::Unknown => todo!(),
                },
                Err(_) => todo!(),
            }
        }
    }
}

/// Target-specific Fatal Error
#[derive(Debug)]
enum TricoreTargetError {
    // ...
    TriggerRemoveFailed(anyhow::Error),
    Fatal,
}

impl From<anyhow::Error> for TricoreTargetError {
    fn from(e: anyhow::Error) -> Self {
        TricoreTargetError::TriggerRemoveFailed(e)
    }
}

impl fmt::Display for TricoreTargetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TricoreTargetError::TriggerRemoveFailed(e) => write!(f, "Trigger remove failed: {}", e),
            TricoreTargetError::Fatal => write!(f, "Fatal error occurred"),
        }
    }
}

impl target::ext::monitor_cmd::MonitorCmd for TricoreTarget<'static> {
    fn handle_monitor_cmd(
        &mut self,
        cmd: &[u8],
        mut out: ConsoleOutput<'_>,
    ) -> Result<(), Self::Error> {
        let cmd = match core::str::from_utf8(cmd) {
            Ok(cmd) => cmd,
            Err(_) => {
                outputln!(out, "command must be valid UTF-8");
                return Ok(());
            }
        };

        match cmd {
            "" => outputln!(out, "Sorry, didn't catch that. Try `monitor ping`!"),
            "ping" => outputln!(out, "pong!"),
            _ => outputln!(out, "I don't know how to handle '{}'", cmd),
        };

        Ok(())
    }
}

impl From<TricoreTargetError> for TargetError<TricoreTargetError> {
    fn from(e: TricoreTargetError) -> Self {
        TargetError::Fatal(e)
    }
}


impl Error for TricoreTargetError {}

impl Target for StaticTricoreTarget {
    type Arch = TricoreV1_6;
    type Error = &'static str;

    #[inline(always)]
    fn base_ops(&mut self) -> target::ext::base::BaseOps<'_, Self::Arch, Self::Error> {
        target::ext::base::BaseOps::SingleThread(self)
    }

    #[inline(always)]
    fn support_breakpoints(&mut self) -> Option<BreakpointsOps<'_, Self>> {
        Some(self)
    }

    #[inline(always)]
    fn support_monitor_cmd(&mut self) -> Option<target::ext::monitor_cmd::MonitorCmdOps<'_, Self>> {
        Some(self)
    }
}

impl SingleThreadBase for StaticTricoreTarget {
    fn read_registers(
        &mut self,
        regs: &mut gdbstub_arch::tricore::reg::TricoreCoreRegs,
    ) -> TargetResult<(), Self> {
        let groups = self
            .core
            .register_groups()
            .map_err(|_| TargetError::Fatal("Can't read register groups"))?;

        let group = groups
            .get_group(0)
            .map_err(|_| TargetError::Fatal("Can't read register groups"))?;

        let register = |name: &str| -> anyhow::Result<u32> {
            let reg = group.register(name).ok_or_else(|| {
                anyhow::Error::msg(format!("Could not find {} register for core", name))
            })?;

            let value = reg
                .read()
                .with_context(|| format!("Cannot read {} register", name))?;
            Ok(value)
        };

        regs.a10 = register("A10").map_err(|_| TargetError::Fatal("Can't read register A10"))?;
        regs.a11 = register("A11").map_err(|_| TargetError::Fatal("Can't read register A11"))?;
        regs.a12 = register("A12").map_err(|_| TargetError::Fatal("Can't read register A12"))?;
        regs.a13 = register("A13").map_err(|_| TargetError::Fatal("Can't read register A13"))?;
        regs.a14 = register("A14").map_err(|_| TargetError::Fatal("Can't read register A14"))?;
        regs.a15 = register("A15").map_err(|_| TargetError::Fatal("Can't read register A15"))?;

        regs.d8 = register("D8").map_err(|_| TargetError::Fatal("Can't read register D8"))?;
        regs.d9 = register("D9").map_err(|_| TargetError::Fatal("Can't read register D9"))?;
        regs.d10 = register("D10").map_err(|_| TargetError::Fatal("Can't read register D10"))?;
        regs.d11 = register("D11").map_err(|_| TargetError::Fatal("Can't read register D11"))?;
        regs.d12 = register("D12").map_err(|_| TargetError::Fatal("Can't read register D12"))?;
        regs.d13 = register("D13").map_err(|_| TargetError::Fatal("Can't read register D13"))?;
        regs.d14 = register("D14").map_err(|_| TargetError::Fatal("Can't read register D14"))?;
        regs.d15 = register("D15").map_err(|_| TargetError::Fatal("Can't read register D15"))?;

        regs.pc = register("PC").map_err(|_| TargetError::Fatal("Can't read register PC"))?;
        regs.pcxi = register("PCXI").map_err(|_| TargetError::Fatal("Can't read register PCXI"))?;
        regs.psw = register("PSW").map_err(|_| TargetError::Fatal("Can't read register PSW"))?;

        Ok(())
    }

    fn write_registers(
        &mut self,
        regs: &gdbstub_arch::tricore::reg::TricoreCoreRegs,
    ) -> TargetResult<(), Self> {
        todo!()
    }

    fn read_addrs(&mut self, start_addr: u32, data: &mut [u8]) -> TargetResult<usize, Self> {
        let bytes = self
            .core
            .read_bytes(start_addr as u64, data.len())
            .map_err(|_| TargetError::Fatal("read_addr failed"))?;
        // .with_context(|| format!("Cannot read from requested address range {:0x} - {:0x}", start_addr, start_addr + data.len()))?;

        data.copy_from_slice(&bytes);

        Ok(bytes.len())
    }

    fn write_addrs(&mut self, start_addr: u32, data: &[u8]) -> TargetResult<(), Self> {
        self.core.write(start_addr as u64,data.to_vec()).map_err(|_|  TargetError::Fatal("Can't write address"))
    }

    #[inline(always)]
    fn support_resume(&mut self) -> Option<SingleThreadResumeOps<'_, Self>> {
        Some(self)
    }

    // fn support_single_register_access(
    //     &mut self,
    // ) -> Option<target::ext::base::single_register_access::SingleRegisterAccessOps<'_, (), Self>> {
    //     todo!()
    // }
}

impl SingleThreadResume for StaticTricoreTarget {
    fn resume(&mut self, signal: Option<Signal>) -> Result<(), Self::Error> {
        if signal.is_some() {
            return Err("no support for continuing with signal");
        }

        self.core.run().map_err(|_| "failed to run core")
    }

    #[inline(always)]
    fn support_single_step(
        &mut self,
    ) -> Option<target::ext::base::singlethread::SingleThreadSingleStepOps<'_, Self>> {
        Some(self)
    }

    #[inline(always)]
    fn support_range_step(
        &mut self,
    ) -> Option<gdbstub::target::ext::base::singlethread::SingleThreadRangeSteppingOps<'_, Self>>
    {
        None
    }
}

impl target::ext::base::singlethread::SingleThreadSingleStep for StaticTricoreTarget {
    fn step(&mut self, signal: Option<Signal>) -> Result<(), Self::Error> {
        if signal.is_some() {
            return Err("no support for stepping with signal");
        }

        self.core.step().map_err(|_| "failed to run core")
    }
}

impl Breakpoints for StaticTricoreTarget {
    // there are several kinds of breakpoints - this target uses software breakpoints
    #[inline(always)]
    fn support_sw_breakpoint(&mut self) -> Option<SwBreakpointOps<'_, Self>> {
        Some(self)
    }
}

impl target::ext::breakpoints::SwBreakpoint for StaticTricoreTarget {
    fn add_sw_breakpoint(
        &mut self,
        addr: u32,
        //todo: refer type from gdbstub_arch
        _kind: usize,
    ) -> TargetResult<bool, Self> {

        let static_core: &'static mut rust_mcd::core::Core<'static> = unsafe {
            std::mem::transmute(&mut self.core)
        };
        
        let trig = static_core.create_breakpoint(rust_mcd::breakpoint::TriggerType::IP, addr as u64, 4);
        
        match trig {
            Ok(trigger) => {
                self.core.download_triggers();
                self.breakpoints.insert(addr, trigger);
                Ok(true)
            }
            Err(e) => Err(TargetError::Fatal("Can't write address")),
        }
    }

    fn remove_sw_breakpoint(
        &mut self,
        addr: u32,
        //todo: refere type from gdbstub_arch
        _kind: usize,
    ) -> TargetResult<bool, Self> {
        if let Some((_, trigger)) = self.breakpoints.remove_entry(&addr) {
            match trigger.remove() {
                Ok(_) => Ok(true),
                Err(e) => Err(TargetError::Fatal("Failed to remove trigger")),
            }
        } else {
            Ok(false) // Address was not found
        }
    }
}
