use heapless::{
    Vec,
    consts::*,
};

use crate::actor::{Actor, ActorContext};
use core::task::{Poll, Context, Waker, RawWaker, RawWakerVTable};
use core::sync::atomic::{AtomicU8, Ordering};
use crate::interrupt::Interrupt;


pub(crate) trait ActiveInterrupt {
    fn on_interrupt(&self);
}

impl<I: Actor + Interrupt> ActiveInterrupt for ActorContext<I> {
    fn on_interrupt(&self) {
        log::info!( "--->");
        unsafe {
            (&mut *self.actor.get()).on_interrupt();
        }
    }
}

struct Interruptable {
    irq: u8,
    interrupt: &'static dyn ActiveInterrupt,
}

impl Interruptable {
    pub fn new(interrupt: &'static dyn ActiveInterrupt, irq: u8) -> Self {
        Self {
            irq,
            interrupt,
        }
    }
}

pub struct InterruptDispatcher {
    interrupts: Vec<Interruptable, U16>,
}

impl InterruptDispatcher {
    pub(crate) fn new() -> Self {
        Self {
            interrupts: Vec::new(),
        }
    }

    pub(crate) fn activate_interrupt<I: ActiveInterrupt>(&mut self, interrupt: &'static I, irq: u8) {
        self.interrupts.push(Interruptable::new(interrupt, irq)).unwrap_or_else( |_| panic!( "too many interrupts" ) );
    }

    #[doc(hidden)]
    pub(crate) fn on_interrupt(&self, irqn: i16) {
        for interrupt in self.interrupts.iter().filter(|e| e.irq == irqn as u8) {
            log::info!("send along irq");
            interrupt.interrupt.on_interrupt();
        }
    }
}
