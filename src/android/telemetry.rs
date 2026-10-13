use std::fs;
use std::path::{Path, PathBuf};
use crate::error::DroidError;

const POWER_SUPPLY_PATH: &str = "/sys/class/power_supply";
const THERMAL_CLASS_PATH: &str = "/sys/class/thermal";
const PROC_MEMINFO_PATH: &str = "/proc/meminfo";
const PROC_UPTIME_PATH: &str = "/proc/uptime";

/// Estado consolidado de telemetría del dispositivo Android / Host Linux
#[derive(Debug, Clone)]
pub struct DeviceTelemetry {
    pub battery: Option<BatteryTelemetry>,
    pub thermal_zones: Vec<ThermalZone>,
    pub max_soc_temp_c: Option<f32>,
    pub memory: Option<MemoryTelemetry>,
    pub uptime_seconds: Option<u64>,
}

/// Métricas de batería extraídas de `/sys/class/power_supply/`
#[derive(Debug, Clone)]
pub struct BatteryTelemetry {
    pub supply_name: String,
    pub capacity_percent: u8,
    pub status: String,
    pub health: Option<String>,
    pub temperature_c: Option<f32>,
    pub voltage_mv: Option<u32>,
    pub current_ma: Option<i32>,
}

/// Representa una zona térmica activa en `/sys/class/thermal/thermal_zone*`
#[derive(Debug, Clone)]
pub struct ThermalZone {
    pub name: String,
    pub sensor_type: String,
    pub temperature_c: f32,
}

/// Uso de memoria del sistema extraído de `/proc/meminfo`
#[derive(Debug, Clone)]
pub struct MemoryTelemetry {
    pub total_mb: u64,
    pub available_mb: u64,
    pub used_mb: u64,
    pub usage_percent: u8,
}

/// Recolecta de forma segura todas las métricas de hardware y sistema disponibles
pub fn collect_telemetry() -> Result<DeviceTelemetry, DroidError> {
    let battery = read_battery_telemetry()?;
    let thermal_zones = read_thermal_zones()?;
    let max_soc_temp_c = find_max_soc_temperature(&thermal_zones);
    let memory = read_memory_telemetry();
    let uptime_seconds = read_uptime_seconds();

    Ok(DeviceTelemetry {
        battery,
        thermal_zones,
        max_soc_temp_c,
        memory,
        uptime_seconds,
    })
}

/// Busca y lee los datos de la batería principal en `/sys/class/power_supply`
pub fn read_battery_telemetry() -> Result<Option<BatteryTelemetry>, DroidError> {
    let base_path = Path::new(POWER_SUPPLY_PATH);
    if !base_path.exists() {
        return Ok(None);
    }

    let Some(battery_dir) = locate_battery_directory(base_path) else {
        return Ok(None);
    };

    let supply_name = battery_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("battery")
        .to_string();

    let capacity_raw = read_sysfs_i64(&battery_dir.join("capacity")).unwrap_or(0);
    let capacity_percent = capacity_raw.clamp(0, 100) as u8;

    let status = read_sysfs_string(&battery_dir.join("status"))
        .unwrap_or_else(|| "Unknown".to_string());

    let health = read_sysfs_string(&battery_dir.join("health"));

    // En Android power_supply, `temp` se expresa habitualmente en décimas de °C (ej. 325 = 32.5°C)
    let temperature_c = read_sysfs_i64(&battery_dir.join("temp"))
        .and_then(normalize_battery_temp_celsius);

    // `voltage_now` suele expresarse en microvoltios (µV) o milivoltios (mV)
    let voltage_mv = read_sysfs_i64(&battery_dir.join("voltage_now"))
        .and_then(normalize_voltage_mv);

    // `current_now` suele expresarse en microamperios (µA) o miliamperios (mA)
    let current_ma = read_sysfs_i64(&battery_dir.join("current_now"))
        .map(normalize_current_ma);

    Ok(Some(BatteryTelemetry {
        supply_name,
        capacity_percent,
        status,
        health,
        temperature_c,
        voltage_mv,
        current_ma,
    }))
}

/// Escanea todas las zonas térmicas en `/sys/class/thermal` filtrando sensores inválidos o apagados
pub fn read_thermal_zones() -> Result<Vec<ThermalZone>, DroidError> {
    let base_path = Path::new(THERMAL_CLASS_PATH);
    if !base_path.exists() {
        return Ok(Vec::new());
    }

    let entries = match fs::read_dir(base_path) {
        Ok(iter) => iter,
        Err(_) => return Ok(Vec::new()),
    };

    let mut zones = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        if !dir_name.starts_with("thermal_zone") {
            continue;
        }

        let Some(sensor_type) = read_sysfs_string(&path.join("type")) else {
            continue;
        };

        let Some(raw_temp) = read_sysfs_i64(&path.join("temp")) else {
            continue;
        };

        if let Some(temperature_c) = normalize_thermal_zone_celsius(raw_temp) {
            zones.push(ThermalZone {
                name: dir_name.to_string(),
                sensor_type,
                temperature_c,
            });
        }
    }

    zones.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(zones)
}

/// Identifica la temperatura máxima entre sensores asociados a CPU, SoC o paquete principal
fn find_max_soc_temperature(zones: &[ThermalZone]) -> Option<f32> {
    let mut max_temp: Option<f32> = None;

    for zone in zones {
        let lower = zone.sensor_type.to_ascii_lowercase();
        let is_cpu_or_soc = lower.contains("cpu")
            || lower.contains("soc")
            || lower.contains("tsens")
            || lower.contains("cluster")
            || lower.contains("ap")
            || lower.contains("pkg")
            || lower.contains("x86_pkg_temp");

        if is_cpu_or_soc {
            max_temp = Some(match max_temp {
                Some(current_max) => current_max.max(zone.temperature_c),
                None => zone.temperature_c,
            });
        }
    }

    // Fallback: si el fabricante usa nombres no estándar, tomar la máxima general razonable
    if max_temp.is_none() {
        for zone in zones {
            max_temp = Some(match max_temp {
                Some(current_max) => current_max.max(zone.temperature_c),
                None => zone.temperature_c,
            });
        }
    }

    max_temp
}

/// Localiza el directorio de la batería en `/sys/class/power_supply`
fn locate_battery_directory(base_path: &Path) -> Option<PathBuf> {
    // 1. Intento directo en rutas estándar de kernels Android
    for candidate in ["battery", "Battery", "BAT0", "BAT1", "bms"] {
        let candidate_path = base_path.join(candidate);
        if candidate_path.exists() {
            return Some(candidate_path);
        }
    }

    // 2. Búsqueda dinámica inspeccionando el archivo `type == "Battery"`
    let entries = fs::read_dir(base_path).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(supply_type) = read_sysfs_string(&path.join("type")) {
            if supply_type.eq_ignore_ascii_case("battery") {
                return Some(path);
            }
        }
    }

    None
}

/// Lee estadísticas de memoria RAM desde `/proc/meminfo`
fn read_memory_telemetry() -> Option<MemoryTelemetry> {
    let content = fs::read_to_string(PROC_MEMINFO_PATH).ok()?;
    let mut mem_total_kb: Option<u64> = None;
    let mut mem_available_kb: Option<u64> = None;
    let mut mem_free_kb: Option<u64> = None;

    for line in content.lines() {
        if let Some(val) = line.strip_prefix("MemTotal:") {
            mem_total_kb = parse_meminfo_kb(val);
        } else if let Some(val) = line.strip_prefix("MemAvailable:") {
            mem_available_kb = parse_meminfo_kb(val);
        } else if let Some(val) = line.strip_prefix("MemFree:") {
            mem_free_kb = parse_meminfo_kb(val);
        }
    }

    let total_kb = mem_total_kb?;
    if total_kb == 0 {
        return None;
    }

    let available_kb = mem_available_kb.or(mem_free_kb).unwrap_or(0);
    let used_kb = total_kb.saturating_sub(available_kb);

    let total_mb = total_kb / 1024;
    let available_mb = available_kb / 1024;
    let used_mb = used_kb / 1024;
    let usage_percent = ((used_kb.saturating_mul(100)) / total_kb).clamp(0, 100) as u8;

    Some(MemoryTelemetry {
        total_mb,
        available_mb,
        used_mb,
        usage_percent,
    })
}

/// Extrae el valor en kB de una línea de `/proc/meminfo`
fn parse_meminfo_kb(raw_field: &str) -> Option<u64> {
    raw_field
        .split_whitespace()
        .next()
        .and_then(|num| num.parse::<u64>().ok())
}

/// Lee el tiempo de actividad del kernel desde `/proc/uptime`
fn read_uptime_seconds() -> Option<u64> {
    let content = fs::read_to_string(PROC_UPTIME_PATH).ok()?;
    let first_token = content.split_whitespace().next()?;
    let secs_str = first_token.split('.').next()?;
    secs_str.parse::<u64>().ok()
}

/// Lee y recorta una cadena de texto desde un nodo `sysfs`
fn read_sysfs_string(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Lee un entero con signo desde un nodo `sysfs`
fn read_sysfs_i64(path: &Path) -> Option<i64> {
    read_sysfs_string(path)?.parse::<i64>().ok()
}

/// Normaliza la lectura de temperatura de una zona térmica a grados Celsius (°C)
fn normalize_thermal_zone_celsius(raw: i64) -> Option<f32> {
    if raw <= 0 {
        return None;
    }

    let celsius = if raw >= 1000 {
        // Miligrados Celsius (ej. 43500 -> 43.5 °C)
        raw as f32 / 1000.0
    } else if raw >= 150 {
        // Décimas de grado Celsius (ej. 435 -> 43.5 °C)
        raw as f32 / 10.0
    } else {
        // Grados Celsius directos (ej. 43 -> 43.0 °C)
        raw as f32
    };

    // Filtrar lecturas espurias de sensores desconectados (ej. > 130°C o < 5°C)
    if (5.0..=130.0).contains(&celsius) {
        Some(celsius)
    } else {
        None
    }
}

/// Normaliza la temperatura de la batería a grados Celsius (°C)
fn normalize_battery_temp_celsius(raw: i64) -> Option<f32> {
    if raw <= 0 {
        return None;
    }

    let celsius = if raw >= 1000 {
        raw as f32 / 1000.0
    } else if raw >= 100 {
        // Estándar Android power_supply: décimas de grado (ej. 312 -> 31.2 °C)
        raw as f32 / 10.0
    } else {
        raw as f32
    };

    if (0.0..=90.0).contains(&celsius) {
        Some(celsius)
    } else {
        None
    }
}

/// Normaliza el voltaje de la batería a milivoltios (mV)
fn normalize_voltage_mv(raw: i64) -> Option<u32> {
    if raw <= 0 {
        return None;
    }

    let mv = if raw > 100_000 {
        // Microvoltios a milivoltios (ej. 4125000 µV -> 4125 mV)
        (raw / 1000) as u32
    } else {
        raw as u32
    };

    Some(mv)
}

/// Normaliza la corriente eléctrica a miliamperios (mA)
fn normalize_current_ma(raw: i64) -> i32 {
    if raw.abs() > 10_000 {
        // Microamperios a miliamperios
        (raw / 1000) as i32
    } else {
        raw as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_thermal_zone_celsius() {
        assert_eq!(normalize_thermal_zone_celsius(45500), Some(45.5));
        assert_eq!(normalize_thermal_zone_celsius(420), Some(42.0));
        assert_eq!(normalize_thermal_zone_celsius(39), Some(39.0));
        assert_eq!(normalize_thermal_zone_celsius(-273000), None);
        assert_eq!(normalize_thermal_zone_celsius(250000), None);
    }

    #[test]
    fn test_normalize_battery_metrics() {
        assert_eq!(normalize_battery_temp_celsius(335), Some(33.5));
        assert_eq!(normalize_voltage_mv(4_200_000), Some(4200));
        assert_eq!(normalize_voltage_mv(4150), Some(4150));
        assert_eq!(normalize_current_ma(-450_000), -450);
    }
}