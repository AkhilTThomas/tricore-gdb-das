use crate::DynResult;
use anyhow::{Context, Result};
use gdbstub::common::Signal;
use gdbstub::target;
use gdbstub::target::ext::base::multithread::{
    MultiThreadBase, MultiThreadResume, MultiThreadResumeOps,
};
use gdbstub::target::ext::breakpoints::{Breakpoints, BreakpointsOps, SwBreakpointOps};
use gdbstub::target::ext::monitor_cmd::ConsoleOutput;
use gdbstub::target::TargetResult;
use gdbstub::target::{Target, TargetError};
use gdbstub_arch::tricore::TricoreV1_6;
use log::{debug, trace};
use rust_mcd::core::{Core, CoreState, Trigger};
use rust_mcd::reset::ResetClass;
mod chip_communication;
use gdbstub::target::ext::monitor_cmd::outputln;
pub mod das;
pub mod elf;
pub mod flash;
pub mod tricore;
use chip_communication::DeviceSelection;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;

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

/// Actions for resuming a core
#[derive(Debug, Copy, Clone)]
pub(crate) enum ResumeAction {
    /// Don't change the state
    Unchanged,
    /// Resume core
    Resume,
    /// Single step core
    Step,
}

/// Target-specific Fatal Error
#[derive(Debug)]
enum TricoreTargetError {
    // ...
    Fatal(String),
    TriggerRemoveFailed(anyhow::Error),
    Str(&'static str),
    String(String),
}

impl fmt::Display for TricoreTargetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TricoreTargetError::Fatal(msg) => write!(f, "Fatal error: {}", msg),
            TricoreTargetError::TriggerRemoveFailed(e) => write!(f, "Trigger remove failed: {}", e),
            TricoreTargetError::Str(s) => write!(f, "{}", s),
            TricoreTargetError::String(s) => write!(f, "{}", s),
        }
    }
}

impl Error for TricoreTargetError {}

impl From<anyhow::Error> for TricoreTargetError {
    fn from(e: anyhow::Error) -> Self {
        TricoreTargetError::TriggerRemoveFailed(e)
    }
}

impl From<&'static str> for TricoreTargetError {
    fn from(s: &'static str) -> Self {
        TricoreTargetError::Str(s)
    }
}

impl From<String> for TricoreTargetError {
    fn from(s: String) -> Self {
        TricoreTargetError::String(s)
    }
}

impl From<TricoreTargetError> for TargetError<&'static str> {
    fn from(error: TricoreTargetError) -> Self {
        match error {
            TricoreTargetError::Str(s) => TargetError::Fatal(s),
            TricoreTargetError::String(s) => TargetError::Fatal("String error occurred"),
            TricoreTargetError::Fatal(s) => TargetError::Fatal("Fatal error occurred"),
            TricoreTargetError::TriggerRemoveFailed(_) => {
                TargetError::Fatal("Trigger remove failed")
            }
        }
    }
}

pub struct TricoreTarget<'a> {
    pub(crate) breakpoints: HashMap<u32, Trigger<'a>>,
    pub(crate) system: rust_mcd::system::System,
    pub(crate) cores: Vec<Core<'a>>,
    /// Resume action to be used upon a continue request
    resume_actions: Vec<ResumeAction>,
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

        let core_count = system.core_count();
        debug!("Detected {:?} core", core_count);

        let mut cores: Vec<Core<'static>> = Vec::with_capacity(core_count);
        let mut resume_actions: Vec<ResumeAction> = Vec::with_capacity(core_count);

        for core_index in 0..core_count {
            let core = system.get_core(core_index)?;
            let system_reset = ResetClass::construct_reset_class(&core, 0);
            core.reset(system_reset, true)?;
            let static_core: Core<'static> =
                unsafe { std::mem::transmute::<Core<'_>, Core<'static>>(core) };
            cores.push(static_core);
            resume_actions.push(ResumeAction::Unchanged);
        }

        Ok(TricoreTarget {
            breakpoints: HashMap::new(),
            system,
            cores,
            resume_actions,
        })
    }

    // run till event
    pub fn run(&mut self, mut poll_incoming_data: impl FnMut() -> bool) -> tricore::RunEvent {
        loop {
            if poll_incoming_data() {
                break tricore::RunEvent::IncomingData;
            }
            for (index, core) in &mut self.cores.iter_mut().enumerate() {
                match core.query_state() {
                    Ok(core_info) => match core_info.state {
                        CoreState::Debug => {
                            let cpu_id = CpuId::try_from(index).expect("Unexpected core index");
                            debug!("Core {:?} in Debug state", index);
                            return tricore::RunEvent::Event(tricore::Event::Break, cpu_id);
                        }
                        CoreState::Custom => todo!(),
                        CoreState::Halted => {
                            let cpu_id = CpuId::try_from(index).expect("Unexpected core index");
                            tricore::RunEvent::Event(tricore::Event::Break, cpu_id);
                        }
                        CoreState::Running => {
                            debug!("Core {:?} Running", index);
                        }
                        CoreState::Unknown => todo!(),
                    },
                    Err(_) => {
                        debug!("What is this weird undocumented state!")
                    }
                }
            }
        }
    }

    fn get_core(&self, tid: Tid) -> Result<&Core<'static>, TricoreTargetError> {
        let core_id = tid_to_cpuid(tid).map_err(TricoreTargetError::Str)?;
        let index = usize::from(core_id);
        self.cores
            .get(index)
            .ok_or_else(|| TricoreTargetError::Fatal("Invalid core index".to_string()))
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

impl Target for StaticTricoreTarget {
    type Arch = TricoreV1_6;
    type Error = &'static str;

    #[inline(always)]
    fn base_ops(&mut self) -> target::ext::base::BaseOps<'_, Self::Arch, Self::Error> {
        target::ext::base::BaseOps::MultiThread(self)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CpuId {
    Cpu0,
    Cpu1,
    Cpu2,
    Cpu3,
    Cpu4,
    Cpu5,
}

pub type Tid = core::num::NonZeroUsize;

pub fn cpuid_to_tid(id: CpuId) -> Tid {
    match id {
        CpuId::Cpu0 => Tid::new(1).unwrap(),
        CpuId::Cpu1 => Tid::new(2).unwrap(),
        CpuId::Cpu2 => Tid::new(3).unwrap(),
        CpuId::Cpu3 => Tid::new(4).unwrap(),
        CpuId::Cpu4 => Tid::new(5).unwrap(),
        CpuId::Cpu5 => Tid::new(6).unwrap(),
    }
}

fn tid_to_cpuid(tid: Tid) -> Result<CpuId, &'static str> {
    match tid.get() {
        1 => Ok(CpuId::Cpu0),
        2 => Ok(CpuId::Cpu1),
        3 => Ok(CpuId::Cpu2),
        4 => Ok(CpuId::Cpu3),
        5 => Ok(CpuId::Cpu4),
        6 => Ok(CpuId::Cpu5),
        _ => Err("specified invalid core"),
    }
}

// Implement TryFrom<usize> for CpuId
impl TryFrom<usize> for CpuId {
    type Error = &'static str;

    fn try_from(index: usize) -> Result<Self, Self::Error> {
        match index {
            0 => Ok(CpuId::Cpu0),
            1 => Ok(CpuId::Cpu1),
            2 => Ok(CpuId::Cpu2),
            3 => Ok(CpuId::Cpu3),
            4 => Ok(CpuId::Cpu4),
            5 => Ok(CpuId::Cpu5),
            _ => Err("Index out of bounds for CpuId"),
        }
    }
}

// Implement From<CpuId> for usize
impl From<CpuId> for usize {
    fn from(id: CpuId) -> Self {
        match id {
            CpuId::Cpu0 => 0,
            CpuId::Cpu1 => 1,
            CpuId::Cpu2 => 2,
            CpuId::Cpu3 => 3,
            CpuId::Cpu4 => 4,
            CpuId::Cpu5 => 5,
        }
    }
}

impl MultiThreadBase for StaticTricoreTarget {
    fn read_registers(
        &mut self,
        regs: &mut gdbstub_arch::tricore::reg::TricoreCoreRegs,
        tid: Tid,
    ) -> TargetResult<(), Self> {
        let core = self.get_core(tid)?;

        // todo: why is this needed?
        _ = core.query_state();

        let groups = core
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
        tid: Tid,
    ) -> TargetResult<(), Self> {
        todo!()
    }

    fn read_addrs(
        &mut self,
        start_addr: u32,
        data: &mut [u8],
        tid: Tid,
    ) -> TargetResult<usize, Self> {
        let core = self.get_core(tid)?;

        let bytes = core
            .read_bytes(start_addr as u64, data.len())
            .map_err(|_| TargetError::Fatal("read_addr failed"))?;
        // .with_context(|| format!("Cannot read from requested address range {:0x} - {:0x}", start_addr, start_addr + data.len()))?;

        data.copy_from_slice(&bytes);

        Ok(bytes.len())
    }

    fn write_addrs(&mut self, start_addr: u32, data: &[u8], tid: Tid) -> TargetResult<(), Self> {
        let core = self.get_core(tid)?;

        core.write(start_addr as u64, data.to_vec())
            .map_err(|_| TricoreTargetError::Fatal("Can't write address".to_string()))?;
        Ok(())
    }

    #[inline(always)]
    fn support_resume(&mut self) -> Option<MultiThreadResumeOps<'_, Self>> {
        Some(self)
    }

    fn list_active_threads(
        &mut self,
        register_thread: &mut dyn FnMut(Tid),
    ) -> Result<(), Self::Error> {
        for (index, _) in self.cores.iter().enumerate() {
            register_thread(Tid::new(index + 1).unwrap());
        }
        Ok(())
    }
}

impl MultiThreadResume for StaticTricoreTarget {
    fn resume(&mut self) -> Result<(), Self::Error> {
        _ = self.cores[1].query_state();

        // iterate through each recoreded resume action and run or step
        for (iter, resume_action) in self.resume_actions.iter().enumerate() {
            let core = &mut self.cores[iter];

            match resume_action {
                ResumeAction::Resume => {
                    trace!("Resumed core {:?}", iter);
                    _ = core
                        .run()
                        .map_err(|_| format!("failed to run core: {}", iter));
                }
                ResumeAction::Step => {
                    trace!("Stepped core {:?}", iter);

                    _ = core
                        .step()
                        .map_err(|_| TargetError::Fatal(format!("failed to run core: {}", iter)));
                }
                ResumeAction::Unchanged => {}
            }
        }

        Ok(())
    }

    #[inline(always)]
    fn support_single_step(
        &mut self,
    ) -> Option<target::ext::base::multithread::MultiThreadSingleStepOps<'_, Self>> {
        Some(self)
    }

    fn set_resume_action_continue(
        &mut self,
        tid: Tid,
        signal: Option<Signal>,
    ) -> Result<(), Self::Error> {
        if signal.is_some() {
            return Err("no support for continuing with signal");
        }
        let core_id = tid_to_cpuid(tid)?;
        let index = usize::from(core_id);
        self.resume_actions[index] = ResumeAction::Resume;

        Ok(())
    }

    fn clear_resume_actions(&mut self) -> Result<(), Self::Error> {
        for resume_action in self.resume_actions.iter_mut() {
            *resume_action = ResumeAction::Resume;
        }
        Ok(())
    }
}

impl target::ext::base::multithread::MultiThreadSingleStep for StaticTricoreTarget {
    fn set_resume_action_step(
        &mut self,
        tid: Tid,
        signal: Option<Signal>,
    ) -> Result<(), Self::Error> {
        if signal.is_some() {
            return Err("no support for stepping with signal");
        }

        let core_id = tid_to_cpuid(tid)?;
        let index = usize::from(core_id);

        self.resume_actions[index] = ResumeAction::Step;

        Ok(())
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
        let static_core: &'static mut rust_mcd::core::Core<'static> =
            unsafe { std::mem::transmute(&mut self.cores[0]) };

        let trig =
            static_core.create_breakpoint(rust_mcd::breakpoint::TriggerType::IP, addr as u64, 4);

        match trig {
            Ok(trigger) => {
                self.cores[0].download_triggers();
                self.breakpoints.insert(addr, trigger);
                Ok(true)
            }
            Err(e) => {
                println!("Can't write to address: {:#01x}", addr);
                Err(TargetError::Fatal("Can't write to address"))
            }
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
