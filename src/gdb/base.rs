use anyhow::{Context, Result};
use gdbstub::{
    common::Tid,
    target::{
        ext::base::multithread::{MultiThreadBase, MultiThreadResumeOps},
        TargetError, TargetResult,
    },
};

use super::{traits::TricoreTargetError, StaticTricoreTarget};

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
        _regs: &gdbstub_arch::tricore::reg::TricoreCoreRegs,
        _tid: Tid,
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
