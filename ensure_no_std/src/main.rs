#![no_std]
#![no_main]

#[allow(unused_imports, clippy::single_component_path_imports)]
use bevy_mod_time_travel;

use core::panic::PanicInfo;

/// This function is called on panic.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    loop {}
}
