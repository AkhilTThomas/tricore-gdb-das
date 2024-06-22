use std::error::Error;
use std::fmt::{self};

use crate::DynResult;
use anyhow::{Context,Result};
use gdbstub::common::Signal;
use gdbstub::target;
use gdbstub::target::ext::base::singlethread::{
    SingleThreadBase, SingleThreadResume, SingleThreadResumeOps,
};
use gdbstub::target::ext::breakpoints::{Breakpoints, BreakpointsOps, SwBreakpointOps};
use gdbstub::target::TargetResult;
use gdbstub::target::{Target, TargetError};
use gdbstub_arch::tricore::TricoreV1_6;
use rust_mcd::core::CoreState;
use rust_mcd::reset::ResetClass;
mod chip_communication;
pub mod das;
pub mod elf;
pub mod flash;

use std::path::PathBuf;
use chip_communication::DeviceSelection;

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
#[derive(Clone)]
pub struct TricoreTarget {
    pub(crate) breakpoints: Vec<u32>,
    pub(crate) system: rust_mcd::system::System,
}

impl TricoreTarget {
    pub fn new(program_elf: Option<&PathBuf>) -> DynResult<TricoreTarget> {
        let mut command_server = chip_communication::ChipCommunication::new()?;
        let scanned_devices = command_server.list_devices()?;

        pretty_print_devices(&scanned_devices);

        command_server.connect(Some(&scanned_devices[0]))?;

        match program_elf {
            Some(program_elf) => {
                println!("Programming via elf: {:?}", program_elf);
                command_server
                    .flash_elf(program_elf)
                    .context("Cannot flash elf")?;
            }
            None => println!("No elf provided..."),
        }

        let system = command_server.get_system()?;
        let core = system.get_core(0)?;
        let system_reset = ResetClass::construct_reset_class(&core, 0);

        // Do we also need to reset the other cores?
        core.reset(system_reset, true)?;

        Ok(TricoreTarget {
            breakpoints: Vec::new(),
            system,
        })
    }

    pub fn get_core_state(self) -> DynResult<CoreState> {
        let core = self.system.get_core(0)?;
        
        let core_info =core.query_state()?;

        Ok(core_info.state)
    }
}

/// Target-specific Fatal Error
#[derive(Debug)]
enum TricoreTargetError {
    // ...
    Fatal,
}

impl From<TricoreTargetError> for TargetError<TricoreTargetError> {
    fn from(e: TricoreTargetError) -> Self {
        TargetError::Fatal(e)
    }
}

impl fmt::Display for TricoreTargetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TricoreTargetError::Fatal => write!(f, "Fatal error occurred"),
            // Handle other variants as needed
        }
    }
}

impl Error for TricoreTargetError {}

impl Target for TricoreTarget {
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
}

impl SingleThreadBase for TricoreTarget {
    fn read_registers(
        &mut self,
        regs: &mut gdbstub_arch::tricore::reg::TricoreCoreRegs,
    ) -> TargetResult<(), Self> {
        let core = self
            .system
            .get_core(0)
            .map_err(|_| TargetError::Fatal("wtf"))?;

        let groups = core
            .register_groups()
            .map_err(|_| TargetError::Fatal("wtf"))?;
        let group = groups.get_group(0).map_err(|_| TargetError::Fatal("wtf"))?;

        let register = |name: &str| -> anyhow::Result<u32> {
            let reg = group.register(name).ok_or_else(|| {
                anyhow::Error::msg(format!("Could not find {} register for core", name))
            })?;

            let value = reg
                .read()
                .with_context(|| format!("Cannot read {} register", name))?;
            Ok(value)
        };

        //regs.a0 = register("A0").map_err(|_| TargetError::Fatal("wtf"))?;
        //regs.a1 = register("A1").map_err(|_| TargetError::Fatal("wtf"))?;
        //regs.a2 = register("A2").map_err(|_| TargetError::Fatal("wtf"))?;
        // regs.a3 = register("A3").map_err(|_| TargetError::Fatal("wtf"))?;
        // regs.a4 = register("A4").map_err(|_| TargetError::Fatal("wtf"))?;
        // regs.a5 = register("A5").map_err(|_| TargetError::Fatal("wtf"))?;
        // regs.a6 = register("A6").map_err(|_| TargetError::Fatal("wtf"))?;
        // regs.a7 = register("A7").map_err(|_| TargetError::Fatal("wtf"))?;
        // regs.a8 = register("A8").map_err(|_| TargetError::Fatal("wtf"))?;
        // regs.a9 = register("A9").map_err(|_| TargetError::Fatal("wtf"))?;
        regs.a10 = register("A10").map_err(|_| TargetError::Fatal("wtf"))?;
        regs.a11 = register("A11").map_err(|_| TargetError::Fatal("wtf"))?;
        regs.a12 = register("A12").map_err(|_| TargetError::Fatal("wtf"))?;
        regs.a13 = register("A13").map_err(|_| TargetError::Fatal("wtf"))?;
        regs.a14 = register("A14").map_err(|_| TargetError::Fatal("wtf"))?;
        regs.a15 = register("A15").map_err(|_| TargetError::Fatal("wtf"))?;
        // regs.d0 = register("D0").map_err(|_| TargetError::Fatal("wtf"))?;
        // regs.d1 = register("D1").map_err(|_| TargetError::Fatal("wtf"))?;
        // regs.d2 = register("D2").map_err(|_| TargetError::Fatal("wtf"))?;
        // regs.d3 = register("D3").map_err(|_| TargetError::Fatal("wtf"))?;
        // regs.d4 = register("D4").map_err(|_| TargetError::Fatal("wtf"))?;
        // regs.d5 = register("D5").map_err(|_| TargetError::Fatal("wtf"))?;
        // regs.d6 = register("D6").map_err(|_| TargetError::Fatal("wtf"))?;
        // regs.d7 = register("D7").map_err(|_| TargetError::Fatal("wtf"))?;
        regs.d8 = register("D8").map_err(|_| TargetError::Fatal("wtf"))?;
        regs.d9 = register("D9").map_err(|_| TargetError::Fatal("wtf"))?;
        regs.d10 = register("D10").map_err(|_| TargetError::Fatal("wtf"))?;
        regs.d11 = register("D11").map_err(|_| TargetError::Fatal("wtf"))?;
        regs.d12 = register("D12").map_err(|_| TargetError::Fatal("wtf"))?;
        regs.d13 = register("D13").map_err(|_| TargetError::Fatal("wtf"))?;
        regs.d14 = register("D14").map_err(|_| TargetError::Fatal("wtf"))?;
        regs.d15 = register("D15").map_err(|_| TargetError::Fatal("wtf"))?;
        //
        regs.pc = register("PC").map_err(|_| TargetError::Fatal("wtf"))?;
        regs.pcxi = register("PCXI").map_err(|_| TargetError::Fatal("wtf"))?;
        regs.psw = register("PSW").map_err(|_| TargetError::Fatal("wtf"))?;
        // regs.lcx = register("LCX").map_err(|_| TargetError::Fatal("wtf"))?;

        Ok(())
    }

    fn write_registers(
        &mut self,
        regs: &gdbstub_arch::tricore::reg::TricoreCoreRegs,
    ) -> TargetResult<(), Self> {
        todo!()
    }

    fn read_addrs(&mut self, start_addr: u32, data: &mut [u8]) -> TargetResult<usize, Self> {
        let core = self
            .system
            .get_core(0)
            .map_err(|_| TargetError::Fatal("wtf"))?;

        let bytes = core
            .read_bytes(start_addr as u64, data.len())
            .map_err(|_| TargetError::Fatal("wtf"))?;
        // .with_context(|| format!("Cannot read from requested address range {:0x} - {:0x}", start_addr, start_addr + data.len()))?;

        data.copy_from_slice(&bytes);

        Ok(bytes.len())
    }

    fn write_addrs(&mut self, start_addr: u32, data: &[u8]) -> TargetResult<(), Self> {
        todo!()
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

impl SingleThreadResume for TricoreTarget {
    fn resume(&mut self, signal: Option<Signal>) -> Result<(), Self::Error> {
        if signal.is_some() {
            return Err("no support for continuing with signal");
        }

        let core_result = self.system.get_core(0).map_err(|_| "failed to get core");

        match core_result {
            Ok(core) => {
                core.run().map_err(|_| "failed to run core")?;
            }
            Err(err) => {
                return Err(err);
            }
        }

        Ok(())
    }

    #[inline(always)]
    fn support_single_step(
        &mut self,
    ) -> Option<target::ext::base::singlethread::SingleThreadSingleStepOps<'_, Self>> {
        Some(self)
    }

    // #[inline(always)]
    // fn support_range_step(
    //     &mut self,
    // ) -> Option<target::ext::base::singlethread::SingleThreadRangeSteppingOps<'_, Self>> {
    //     todo!()
    // }
}

impl target::ext::base::singlethread::SingleThreadSingleStep for TricoreTarget {
    fn step(&mut self, signal: Option<Signal>) -> Result<(), Self::Error> {
        if signal.is_some() {
            return Err("no support for stepping with signal");
        }
        let core_result = self.system.get_core(0).map_err(|_| "failed to get core");

        match core_result {
            Ok(core) => {
                core.step().map_err(|_| "failed to run core")?;
            }
            Err(err) => {
                return Err(err);
            }
        }

        Ok(())
    }
}

impl Breakpoints for TricoreTarget {
    // there are several kinds of breakpoints - this target uses software breakpoints
    #[inline(always)]
    fn support_sw_breakpoint(&mut self) -> Option<SwBreakpointOps<'_, Self>> {
        Some(self)
    }
}

impl target::ext::breakpoints::SwBreakpoint for TricoreTarget {
    fn add_sw_breakpoint(
        &mut self,
        addr: u32,
        //todo: refer type from gdbstub_arch
        _kind: usize,
    ) -> TargetResult<bool, Self> {
        self.breakpoints.push(addr);

        let core_result = self.system.get_core(0).map_err(|_| "failed to get core");

        match core_result {
            Ok(core) => {
                println!("Breakpoint set!");
                _ = core
                    .create_breakpoint(rust_mcd::breakpoint::TriggerType::IP, addr as u64, 4)
                    .map_err(|_| "failed to run core");
                core.download_triggers();
            }
            Err(err) => {
                return Err(TargetError::Fatal(err));
            }
        }
        Ok(true)
    }

    fn remove_sw_breakpoint(
        &mut self,
        addr: u32,
        //todo: refere type from gdbstub_arch
        _kind: usize,
    ) -> TargetResult<bool, Self> {
        match self.breakpoints.iter().position(|x| *x == addr) {
            None => return Ok(false),
            Some(pos) => self.breakpoints.remove(pos),
        };

        Ok(true)
    }
}
