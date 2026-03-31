use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::channel::Channel;
use embassy_sync::watch::Watch;
use core::cell::Cell;

#[derive(Clone, Copy, defmt::Format, PartialEq)]
pub enum Phase {
    Homing,
    Running,
    /// Unrecoverable hardware fault; payload is a short static message.
    Fault(&'static str),
}

#[derive(Clone, Copy, defmt::Format)]
pub struct RotatorState {
    pub target_az: f32,
    pub target_el: f32,
    pub current_az: f32,
    pub current_el: f32,
    pub moving: bool,
    pub link_up: bool,
    pub phase: Phase,
}

impl Default for RotatorState {
    fn default() -> Self {
        Self {
            target_az: 0.0,
            target_el: 0.0,
            current_az: 0.0,
            current_el: 0.0,
            moving: false,
            link_up: false,
            phase: Phase::Homing,
        }
    }
}

#[derive(Clone, Copy, defmt::Format)]
pub enum RotatorCmd {
    GoTo { az: f32, el: f32 },
    Stop,
}

/// Operator-configurable soft travel limits.  Defaults span the full range.
/// Written by rotctld/easycom tasks, read by motor_task on every GoTo.
#[derive(Clone, Copy)]
pub struct SoftLimits {
    pub az_min: f32,
    pub az_max: f32,
    pub el_min: f32,
    pub el_max: f32,
}

impl SoftLimits {
    pub const fn default() -> Self {
        Self { az_min: 0.0, az_max: 360.0, el_min: 0.0, el_max: 180.0 }
    }
}

pub static LIMITS: Mutex<CriticalSectionRawMutex, Cell<SoftLimits>> =
    Mutex::new(Cell::new(SoftLimits::default()));

pub static STATE: Watch<CriticalSectionRawMutex, RotatorState, 4> = Watch::new();
pub static CMD: Channel<CriticalSectionRawMutex, RotatorCmd, 4> = Channel::new();
