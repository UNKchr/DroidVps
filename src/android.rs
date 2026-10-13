pub mod env;
pub mod network;
pub mod power;
pub mod selinux;
pub mod telemetry;

pub use env::{
    configure_boot_autostart,
    ensure_root_privileges,
    install_termux_wrappers_in,
    is_boot_autostart_enabled,
    remove_boot_autostart,
    setup_standalone_environment,
};
pub use telemetry::{
    collect_telemetry,
    BatteryTelemetry,
    DeviceTelemetry,
    MemoryTelemetry,
    ThermalZone,
};