use crate::DynResult;
use anyhow::{Context, Result};
use gdbstub::common::Tid;
use gdbstub::target;
use gdbstub::target::ext::breakpoints::BreakpointsOps;

use chip_communication::DeviceSelection;
use gdbstub::target::Target;
use gdbstub_arch::tricore::TricoreV1_6;
use log::debug;
use rust_mcd::core::{Core, CoreState, Trigger};
use rust_mcd::reset::ResetClass;
use std::collections::HashMap;
use traits::TricoreTargetError;

use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;

mod base;
mod breakpoints;
mod chip_communication;
mod das;
mod elf;
mod extended_mode;
mod flash;
mod monitor;
mod resume;
mod traits;
pub mod tricore;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CpuId {
    Cpu0,
    Cpu1,
    Cpu2,
    Cpu3,
    Cpu4,
    Cpu5,
}

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

pub struct TricoreTarget<'a> {
    pub(crate) breakpoints: HashMap<u32, Vec<Trigger<'a>>>,
    #[warn(dead_code)]
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

        sleep(Duration::from_secs(2));

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

    pub fn restart(&mut self) {
        for core in &mut self.cores.iter_mut() {
            let system_reset = ResetClass::construct_reset_class(core, 0);
            _ = core.reset(system_reset, true);
        }
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
                            debug!("Core: {:?} halted by breakpoint", cpu_id);
                            return tricore::RunEvent::Event(tricore::Event::Break, cpu_id);
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

    pub fn halt(&mut self) {
        for core in &mut self.cores.iter_mut() {
            _ = core.stop();
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

    #[inline(always)]
    fn support_extended_mode(
        &mut self,
    ) -> Option<target::ext::extended_mode::ExtendedModeOps<'_, Self>> {
        Some(self)
    }
}
