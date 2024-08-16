use gdbstub::target::{
    self,
    ext::breakpoints::{Breakpoints, SwBreakpointOps},
    TargetError, TargetResult,
};

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
            Err(_) => {
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
                Err(_) => Err(TargetError::Fatal("Failed to remove trigger")),
            }
        } else {
            Ok(false) // Address was not found
        }
    }
}
