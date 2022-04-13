#![no_std]
#![no_main]
#![macro_use]
#![feature(generic_associated_types)]
#![feature(type_alias_impl_trait)]

use drogue_device::bsp::boards::nrf52::adafruit_feather_nrf52840::*;
use drogue_device::drivers::ble::gatt::dfu::FirmwareGattService;
use drogue_device::firmware::serial::SerialUpdater;
use drogue_device::firmware::FirmwareManager;
use drogue_device::flash::{FlashState, SharedFlash};
use drogue_device::ActorContext;
use drogue_device::Board;
use embassy::executor::Spawner;
use embassy::time::{Duration, Timer};
use embassy::util::Forever;
use embassy_boot_nrf::updater;
use embassy_nrf::config::Config;
use embassy_nrf::interrupt;
use embassy_nrf::interrupt::Priority;
use embassy_nrf::uarte::{self, Uarte};
use embassy_nrf::{
    gpio::{AnyPin, Output},
    Peripherals,
};
use nrf_softdevice::ble::gatt_server;
use nrf_softdevice::{raw, Flash, Softdevice};

#[cfg(feature = "panic-probe")]
use panic_probe as _;

#[cfg(feature = "nrf-softdevice-defmt-rtt")]
use nrf_softdevice_defmt_rtt as _;

#[cfg(feature = "panic-reset")]
use panic_reset as _;

const FIRMWARE_VERSION: &str = env!("CARGO_PKG_VERSION");
const FIRMWARE_REVISION: Option<&str> = option_env!("REVISION");

mod gatt;
pub use gatt::*;

// Application must run at a lower priority than softdevice
fn config() -> Config {
    let mut config = embassy_nrf::config::Config::default();
    config.gpiote_interrupt_priority = Priority::P2;
    config.time_interrupt_priority = Priority::P2;
    config
}

#[embassy::main(config = "config()")]
async fn main(s: Spawner, p: Peripherals) {
    let board = AdafruitFeatherNrf52840::new(p);

    // Spawn the underlying softdevice task
    let sd = enable_softdevice();
    s.spawn(softdevice_task(sd)).unwrap();

    let version = FIRMWARE_REVISION.unwrap_or(FIRMWARE_VERSION);
    defmt::info!("Running firmware version {}", version);

    // Watchdog will prevent bootloader from resetting. If your application hangs for more than 5 seconds
    // (depending on bootloader config), it will enter bootloader which may swap the application back.
    s.spawn(watchdog_task()).unwrap();

    // The flash peripheral is special when running with softdevice
    let flash = Flash::take(sd);
    static FLASH: FlashState<Flash> = FlashState::new();
    let flash = FLASH.initialize(flash);

    // The SerialUpdater actor follows a fixed frame protocol that updates
    // firmware using the DFU actor
    let irq = interrupt::take!(UARTE0_UART0);
    let mut config = uarte::Config::default();
    config.parity = uarte::Parity::EXCLUDED;
    config.baudrate = uarte::Baudrate::BAUD115200;
    let uart = Uarte::new(board.uarte0, irq, board.rx, board.tx, config);
    let (tx, rx) = uart.split();

    // The updater is the 'application' part of the bootloader that knows where bootloader
    // settings and the firmware update partition is located based on memory.x linker script.
    let mut serial = SerialUpdater::new(
        tx,
        rx,
        FirmwareManager::new(flash.clone(), updater::new()),
        version.as_bytes(),
    );

    // Create a BLE GATT service that is capable of updating firmware
    static GATT: Forever<GattServer> = Forever::new();
    let server = GATT.put(gatt_server::register(sd).unwrap());

    // Wires together the GATT service and the DFU actor
    static UPDATER: ActorContext<
        FirmwareGattService<'static, FirmwareManager<SharedFlash<Flash>>>,
        4,
    > = ActorContext::new();
    let updater = UPDATER.mount(
        s,
        FirmwareGattService::new(
            &server.firmware,
            // The updater is the 'application' part of the bootloader that knows where bootloader
            // settings and the firmware update partition is located based on memory.x linker script.
            FirmwareManager::new(flash, updater::new()),
            version.as_bytes(),
            64,
        )
        .unwrap(),
    );

    // Starts the bluetooth advertisement and GATT server
    s.spawn(bluetooth_task(sd, server, updater)).unwrap();

    // Finally, a blinker application.
    s.spawn(blinker(board.blue_led)).unwrap();

    // Run the serial updater in the main task
    serial.run().await;
}

const BLINK_INTERVAL: Duration = Duration::from_millis(300);

#[embassy::task]
async fn blinker(mut led: Output<'static, AnyPin>) {
    loop {
        Timer::after(BLINK_INTERVAL).await;
        led.set_low();
        Timer::after(BLINK_INTERVAL).await;
        led.set_high();
    }
}

#[embassy::task]
async fn softdevice_task(sd: &'static Softdevice) {
    sd.run().await;
}

// Keeps our system alive
#[embassy::task]
async fn watchdog_task() {
    let mut handle = unsafe { embassy_nrf::wdt::WatchdogHandle::steal(0) };
    loop {
        handle.pet();
        Timer::after(Duration::from_secs(2)).await;
    }
}

fn enable_softdevice() -> &'static Softdevice {
    let config = nrf_softdevice::Config {
        clock: Some(raw::nrf_clock_lf_cfg_t {
            source: raw::NRF_CLOCK_LF_SRC_RC as u8,
            rc_ctiv: 4,
            rc_temp_ctiv: 2,
            accuracy: 7,
        }),
        conn_gap: Some(raw::ble_gap_conn_cfg_t {
            conn_count: 2,
            event_length: 24,
        }),
        conn_gatt: Some(raw::ble_gatt_conn_cfg_t { att_mtu: 256 }),
        gatts_attr_tab_size: Some(raw::ble_gatts_cfg_attr_tab_size_t {
            attr_tab_size: 32768,
        }),
        gap_role_count: Some(raw::ble_gap_cfg_role_count_t {
            adv_set_count: 1,
            periph_role_count: 3,
            central_role_count: 3,
            central_sec_count: 0,
            _bitfield_1: raw::ble_gap_cfg_role_count_t::new_bitfield_1(0),
        }),
        gap_device_name: Some(raw::ble_gap_cfg_device_name_t {
            p_value: b"DrogueDfu" as *const u8 as _,
            current_len: 9,
            max_len: 9,
            write_perm: unsafe { core::mem::zeroed() },
            _bitfield_1: raw::ble_gap_cfg_device_name_t::new_bitfield_1(
                raw::BLE_GATTS_VLOC_STACK as u8,
            ),
        }),
        ..Default::default()
    };
    Softdevice::enable(&config)
}
