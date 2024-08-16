use anyhow::Result;
use gdbstub::{
    common::Tid,
    target::{
        ext::base::multithread::{MultiThreadBase, MultiThreadResumeOps},
        TargetError, TargetResult,
    },
};
use log::debug;

use super::StaticTricoreTarget;

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

        let read_register = |name: &str| -> TargetResult<u32, Self> {
            group
                .register(name)
                .ok_or_else(|| TargetError::Fatal("Could not find {} register"))?
                .read()
                .map_err(|_| TargetError::Fatal("Can't read register"))
        };

        let register_names = [
            "A10", "A11", "A12", "A13", "A14", "A15", "D8", "D9", "D10", "D11", "D12", "D13",
            "D14", "D15", "PC", "PCXI", "PSW",
        ];

        for &name in &register_names {
            let value = read_register(name)?;
            match name {
                "A10" => regs.a10 = value,
                "A11" => regs.a11 = value,
                "A12" => regs.a12 = value,
                "A13" => regs.a13 = value,
                "A14" => regs.a14 = value,
                "A15" => regs.a15 = value,
                "D8" => regs.d8 = value,
                "D9" => regs.d9 = value,
                "D10" => regs.d10 = value,
                "D11" => regs.d11 = value,
                "D12" => regs.d12 = value,
                "D13" => regs.d13 = value,
                "D14" => regs.d14 = value,
                "D15" => regs.d15 = value,
                "PC" => regs.pc = value,
                "PCXI" => regs.pcxi = value,
                "PSW" => regs.psw = value,
                _ => unreachable!(),
            }
        }

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
            .map_err(|_| {
                debug!(
                    "Cannot read from requested address range {:0x} - {:0x}",
                    start_addr,
                    start_addr + data.len() as u32
                );
                TargetError::NonFatal
            })?;

        data.copy_from_slice(&bytes);

        Ok(bytes.len())
    }

    fn write_addrs(&mut self, start_addr: u32, data: &[u8], tid: Tid) -> TargetResult<(), Self> {
        let core = self.get_core(tid)?;

        core.write(start_addr as u64, data.to_vec()).map_err(|_| {
            debug!("Cannot write to addr {:0x} ", start_addr);
            TargetError::NonFatal
        })?;
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
