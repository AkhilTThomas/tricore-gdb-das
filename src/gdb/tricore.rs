use super::CpuId;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Event {
    DoneStep,
    Halted,
    Break,
    WatchWrite(u32),
    WatchRead(u32),
}

pub enum RunEvent {
    Event(Event, CpuId),
    IncomingData,
}
