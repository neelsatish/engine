//! System load for the corner readout: CPU and RAM through sysinfo, and the NVIDIA GPU's load, memory and temperature
//! through NVML (the driver's own monitoring library). CPU temperature isn't readable on Windows without admin.

use nvml_wrapper::{enum_wrappers::device::TemperatureSensor, Nvml};
use serde::Serialize;
use std::sync::Mutex;
use sysinfo::System;

#[derive(Debug, Default, Serialize)]
pub struct Load {
    /// Percent of all cores.
    pub cpu: f32,
    pub ram_used_gb: f32,
    pub ram_total_gb: f32,
    pub gpu: Option<Gpu>,
}

#[derive(Debug, Serialize)]
pub struct Gpu {
    pub name: String,
    pub load: u32,
    pub vram_used_gb: f32,
    pub vram_total_gb: f32,
    pub temp_c: u32,
}

pub struct Monitor {
    sys: Mutex<Option<System>>,
    nvml: Option<Nvml>,
}

impl Monitor {
    pub fn new() -> Self {
        Self { sys: Mutex::new(System::new().ok()), nvml: Nvml::init().ok() }
    }

    /// Reads the current load. CPU percent is measured since the previous read, so poll every second or two.
    pub fn read(&self) -> Load {
        let mut load = Load::default();
        if let Some(sys) = self.sys.lock().unwrap().as_mut() {
            sys.refresh_cpu_usage();
            sys.refresh_memory();
            load.cpu = sys.global_cpu_usage();
            load.ram_used_gb = sys.used_memory() as f32 / 1e9;
            load.ram_total_gb = sys.total_memory() as f32 / 1e9;
        }
        load.gpu = self.nvml.as_ref().and_then(|nvml| {
            let device = nvml.device_by_index(0).ok()?;
            let memory = device.memory_info().ok()?;
            Some(Gpu {
                name: device.name().unwrap_or_default().replace("NVIDIA ", "").replace("GeForce ", ""),
                load: device.utilization_rates().map(|u| u.gpu).unwrap_or(0),
                vram_used_gb: memory.used as f32 / 1e9,
                vram_total_gb: memory.total as f32 / 1e9,
                temp_c: device.temperature(TemperatureSensor::Gpu).unwrap_or(0),
            })
        });
        load
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_this_pc() {
        let m = Monitor::new();
        let _ = m.read(); // the first CPU reading is a baseline
        std::thread::sleep(std::time::Duration::from_millis(250));
        let load = m.read();
        assert!(load.ram_total_gb > 0.5, "{load:?}");
        assert!((0.0..=100.0).contains(&load.cpu));
    }
}
