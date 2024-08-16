use super::StaticTricoreTarget;
use gdbstub::{
    common::Pid,
    target::{
        self,
        ext::extended_mode::{Args, AttachKind, ShouldTerminate},
        TargetResult,
    },
};

impl target::ext::extended_mode::ExtendedMode for StaticTricoreTarget {
    fn kill(&mut self, pid: Option<Pid>) -> TargetResult<ShouldTerminate, Self> {
        eprintln!("GDB sent a kill request for pid {:?}", pid);
        Ok(ShouldTerminate::No)
    }

    fn restart(&mut self) -> Result<(), Self::Error> {
        eprintln!("GDB sent a restart request");
        self.restart();
        Ok(())
    }

    fn attach(&mut self, pid: Pid) -> TargetResult<(), Self> {
        eprintln!("GDB attached to a process with PID {}", pid);
        Ok(())
    }

    fn run(&mut self, filename: Option<&[u8]>, args: Args<'_, '_>) -> TargetResult<Pid, Self> {
        eprintln!(
            "GDB tried to run a new process with filename {:?}, and args {:?}",
            filename, args
        );

        // when running in single-threaded mode, this PID can be anything
        Ok(Pid::new(1337).unwrap())
    }

    fn query_if_attached(&mut self, pid: Pid) -> TargetResult<AttachKind, Self> {
        eprintln!(
            "GDB queried if it was attached to a process with PID {}",
            pid
        );
        Ok(AttachKind::Attach)
    }
}
