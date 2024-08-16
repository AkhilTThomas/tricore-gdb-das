use gdbstub::target::{
    self,
    ext::breakpoints::{Breakpoints, SwBreakpointOps},
    TargetError, TargetResult,
};
use log::debug;
use rust_mcd::core::Trigger;

use super::StaticTricoreTarget;

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
        //this is strange
        let core_count = self.system.core_count();

        let mut triggers = <Vec<Trigger>>::new();

        for idx in 0..core_count {
            let static_core: &'static mut rust_mcd::core::Core<'static> =
                unsafe { std::mem::transmute(&mut self.cores[idx]) };

            let trig = static_core.create_breakpoint(
                rust_mcd::breakpoint::TriggerType::IP,
                addr as u64,
                4,
            );

            match trig {
                Ok(trigger) => {
                    self.cores[idx].download_triggers();
                    triggers.push(trigger);
                }
                Err(_) => {
                    debug!("Can't set breakpoint at address: {:#01x}", addr);
                    return Err(TargetError::Fatal(stringify!(
                        "Can't set breakpoint at address: {:#01x}",
                        addr
                    )));
                }
            }
        }
        self.breakpoints.insert(addr, triggers);

        Ok(true)
    }

    fn remove_sw_breakpoint(
        &mut self,
        addr: u32,
        //todo: refere type from gdbstub_arch
        _kind: usize,
    ) -> TargetResult<bool, Self> {
        if let Some(triggers) = self.breakpoints.remove(&addr) {
            for trigger in triggers {
                match trigger.remove() {
                    Ok(_) => debug!("Removed breakpoint at addr {:#01x}", addr),
                    Err(_) => return Err(TargetError::Fatal("Failed to remove trigger")),
                }
            }
        }
        Ok(true)
    }
}
