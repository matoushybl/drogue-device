#![no_std]
#![no_main]
#![macro_use]
#![feature(generic_associated_types)]
#![feature(type_alias_impl_trait)]

use defmt_rtt as _;
use panic_probe as _;

use drogue_device::{
    bsp::boards::nrf52::adafruit_feather_nrf52840::AdafruitFeatherNrf52840,
    drivers::led::neopixel::{
        filter::{CyclicBrightness, Filter, Gamma},
        rgbw::{NeoPixelRgbw, BLUE},
    },
    Board,
};
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};

const STEP_SIZE: u8 = 2;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_nrf::init(Default::default());
    let board = AdafruitFeatherNrf52840::new(p);
    let mut neopixel = defmt::unwrap!(NeoPixelRgbw::<'_, _, 1>::new(board.pwm0, board.neopixel));

    let cyclic = CyclicBrightness::new(1, 254, STEP_SIZE);
    let mut filter = cyclic.and(Gamma);
    loop {
        neopixel.set_with_filter(&[BLUE], &mut filter).await.ok();
        Timer::after(Duration::from_millis(20)).await;
    }
}
