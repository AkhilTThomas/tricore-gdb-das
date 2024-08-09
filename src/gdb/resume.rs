use gdbstub::{
    common::{Signal, Tid},
    target::{ext::base::multithread::MultiThreadResume, TargetError},
};
use log::trace;

use super::{tid_to_cpuid, ResumeAction, StaticTricoreTarget};

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
    ) -> Option<gdbstub::target::ext::base::multithread::MultiThreadSingleStepOps<'_, Self>> {
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

impl gdbstub::target::ext::base::multithread::MultiThreadSingleStep for StaticTricoreTarget {
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
