//! Dev tool: lists the active playback devices as the output picker sees them.

use pebbly::platform::audio::{list_outputs, output_volume, picker_order};

fn main() {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
    }
    let devices = list_outputs();
    for i in picker_order(&devices) {
        let d = &devices[i];
        println!("{} {:?} {:?} {:?} vol {:?}", if d.default { "*" } else { " " }, d.name, d.kind, d.bus, output_volume(&d.id));
    }
}
