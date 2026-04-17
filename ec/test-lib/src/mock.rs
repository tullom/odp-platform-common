use crate::{BatterySource, ErrorType, RtcSource, ThermalSource, Threshold, common};
use battery_service_messages::{
    BatteryState, BatterySwapCapability, BatteryTechnology, BixFixedStrings, BstReturn, PowerUnit,
};
use embedded_mcu_hal::time::{Datetime, Month, UncheckedDatetime};
use std::sync::{
    Mutex, OnceLock,
    atomic::Ordering,
    atomic::{AtomicI64, AtomicU32},
};
use std::time::Instant;
use time_alarm_service_messages::{
    AcpiDaylightSavingsTimeStatus, AcpiTimeZone, AcpiTimeZoneOffset, AcpiTimerId, AcpiTimestamp,
    AlarmExpiredWakePolicy, AlarmTimerSeconds, TimeAlarmDeviceCapabilities, TimerStatus,
};

/// Errors produced by mock data source operations.
#[derive(Debug)]
pub enum Error {
    /// Data validation failed (invalid enum discriminant, malformed field, etc.)
    InvalidData,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidData => write!(f, "Invalid data"),
        }
    }
}

impl std::error::Error for Error {}

impl crate::Error for Error {
    fn kind(&self) -> crate::ErrorKind {
        match self {
            Self::InvalidData => crate::ErrorKind::InvalidData,
        }
    }
}

static SET_RPM: AtomicI64 = AtomicI64::new(-1);
static SAMPLE: OnceLock<Mutex<(i64, i64)>> = OnceLock::new();
static CURRENT_TEMP_DK: AtomicI64 = AtomicI64::new(2732);
static FAN_STATE: OnceLock<Mutex<FanState>> = OnceLock::new();

/// Tracks a smooth RPM ramp between two values over a given duration.
struct FanState {
    current_rpm: f64,
    target_rpm: f64,
    ramp_start_rpm: f64,
    ramp_start: Instant,
    ramp_secs: f64,
}

impl FanState {
    fn new() -> Self {
        Self {
            current_rpm: 0.0,
            target_rpm: 0.0,
            ramp_start_rpm: 0.0,
            ramp_start: Instant::now(),
            ramp_secs: 0.0,
        }
    }

    /// Begin ramping toward `target` over `duration` seconds.
    /// No-op if the target hasn't changed.
    fn set_target(&mut self, target: f64, duration: f64) {
        if (self.target_rpm - target).abs() > f64::EPSILON {
            self.ramp_start_rpm = self.current_rpm;
            self.target_rpm = target;
            self.ramp_start = Instant::now();
            self.ramp_secs = duration;
        }
    }

    /// Return the current RPM, linearly interpolated along the active ramp.
    fn rpm(&mut self) -> f64 {
        if self.ramp_secs <= 0.0 {
            self.current_rpm = self.target_rpm;
        } else {
            let t = (self.ramp_start.elapsed().as_secs_f64() / self.ramp_secs).clamp(0.0, 1.0);
            self.current_rpm = self.ramp_start_rpm + (self.target_rpm - self.ramp_start_rpm) * t;
        }
        self.current_rpm
    }
}

#[derive(Default, Copy, Clone)]
pub struct Mock {
    rtc: MockRtc,
}

impl Mock {
    pub fn new() -> Self {
        Default::default()
    }
}

impl ErrorType for Mock {
    type Error = Error;
}

impl ThermalSource for Mock {
    fn get_temperature(&self) -> Result<f64, Self::Error> {
        let mut sample = SAMPLE.get_or_init(|| Mutex::new((2732, 1))).lock().unwrap();

        sample.0 += 10 * sample.1;
        if sample.0 >= 3232 || sample.0 <= 2732 {
            sample.1 *= -1;
        }

        CURRENT_TEMP_DK.store(sample.0, Ordering::Relaxed);
        Ok(common::dk_to_c(sample.0 as u32))
    }

    fn get_rpm(&self) -> Result<f64, Self::Error> {
        // If the user explicitly set an RPM, honour it.
        let set_rpm = SET_RPM.load(Ordering::Relaxed);
        if set_rpm >= 0 {
            return Ok(set_rpm as f64);
        }

        let temp_c = common::dk_to_c(CURRENT_TEMP_DK.load(Ordering::Relaxed) as u32);
        let max_rpm = self.get_max_rpm()?;

        // Target RPM and ramp duration based on temperature thresholds.
        let (target, ramp_secs) = if temp_c >= 44.0 {
            (max_rpm, 3.5)
        } else if temp_c >= 40.0 {
            (3500.0, 4.0)
        } else if temp_c >= 28.0 {
            (1500.0, 2.0)
        } else {
            (0.0, 2.0)
        };

        let mut fan = FAN_STATE.get_or_init(|| Mutex::new(FanState::new())).lock().unwrap();
        fan.set_target(target, ramp_secs);
        Ok(fan.rpm())
    }

    fn get_min_rpm(&self) -> Result<f64, Self::Error> {
        Ok(0.0)
    }

    fn get_max_rpm(&self) -> Result<f64, Self::Error> {
        Ok(6000.0)
    }

    fn get_threshold(&self, threshold: Threshold) -> Result<f64, Self::Error> {
        match threshold {
            Threshold::On => Ok(28.0),
            Threshold::Ramping => Ok(40.0),
            Threshold::Max => Ok(44.0),
        }
    }

    fn set_rpm(&self, rpm: f64) -> Result<(), Self::Error> {
        SET_RPM.store(rpm as i64, Ordering::Relaxed);
        Ok(())
    }
}

impl BatterySource for Mock {
    fn get_bst(&self) -> Result<BstReturn, Self::Error> {
        static STATE: AtomicU32 = AtomicU32::new(2);
        const MAX_CAPACITY: u32 = 10000;
        static CAPACITY: AtomicU32 = AtomicU32::new(0);
        const RATE: u32 = 1000;

        let state = STATE.load(Ordering::Relaxed);
        let capacity = CAPACITY.load(Ordering::Relaxed);
        let mut new_capacity = capacity;

        // We are only using atomics to satisfy borrow-checker
        // Thus we update non-atomically for simplicity
        if state == 2 {
            new_capacity += RATE;
            if new_capacity > MAX_CAPACITY {
                STATE.store(1, Ordering::Relaxed);
            }
        } else {
            new_capacity -= RATE;
            if new_capacity < RATE {
                STATE.store(2, Ordering::Relaxed);
            }
        }
        CAPACITY.store(new_capacity.clamp(0, MAX_CAPACITY), Ordering::Relaxed);

        Ok(BstReturn {
            battery_state: BatteryState::from_bits(state).ok_or(Error::InvalidData)?,
            battery_present_rate: 3839,
            battery_remaining_capacity: capacity,
            battery_present_voltage: 12569,
        })
    }

    fn get_bix(&self) -> Result<BixFixedStrings, Self::Error> {
        Ok(BixFixedStrings {
            revision: 1,
            power_unit: PowerUnit::MilliWatts,
            design_capacity: 10000,
            last_full_charge_capacity: 9890,
            battery_technology: BatteryTechnology::Primary,
            design_voltage: 13000,
            design_cap_of_warning: 5000,
            design_cap_of_low: 3000,
            cycle_count: 1337,
            measurement_accuracy: 80000,
            max_sampling_time: 42,
            min_sampling_time: 7,
            max_averaging_interval: 5,
            min_averaging_interval: 1,
            battery_capacity_granularity_1: 10,
            battery_capacity_granularity_2: 10,
            model_number: [b'4', b'2', b'.', b'0', 0, 0, 0, 0],
            serial_number: [b'1', b'2', b'3', b'-', b'4', b'5', 0, 0],
            battery_type: [b'L', b'i', b'-', b'o', b'n', 0, 0, 0],
            oem_info: [b'B', b'a', b't', b'B', b'r', b'o', b's', 0],
            battery_swapping_capability: BatterySwapCapability::ColdSwappable,
        })
    }

    fn set_btp(&self, _trippoint: u32) -> Result<(), Self::Error> {
        // Do nothing for mock
        Ok(())
    }
}

#[derive(Copy, Clone)]
struct MockRtc {
    time: AcpiTimestamp,
    timers: [MockRtcTimer; 2],
}

#[derive(Copy, Clone)]
struct MockRtcTimer {
    value: AlarmTimerSeconds,
    wake_policy: AlarmExpiredWakePolicy,
    timer_status: TimerStatus,
}

impl Default for MockRtcTimer {
    fn default() -> Self {
        Self {
            value: AlarmTimerSeconds(0),
            wake_policy: AlarmExpiredWakePolicy::INSTANTLY,
            timer_status: TimerStatus(0),
        }
    }
}

impl MockRtc {
    fn new() -> Self {
        Self {
            time: AcpiTimestamp {
                datetime: Datetime::new(UncheckedDatetime {
                    year: 2026,
                    month: Month::January,
                    day: 1,
                    ..Default::default()
                })
                .expect("statically known valid datetime"),
                time_zone: AcpiTimeZone::MinutesFromUtc(
                    AcpiTimeZoneOffset::new(-8 * 60).expect("statically known valid timezone"),
                ),
                dst_status: AcpiDaylightSavingsTimeStatus::NotObserved,
            },
            timers: [MockRtcTimer::default(); 2],
        }
    }

    fn get_timer(&self, timer_id: AcpiTimerId) -> &MockRtcTimer {
        &self.timers[timer_id as usize]
    }
}

impl Default for MockRtc {
    fn default() -> Self {
        Self::new()
    }
}

impl RtcSource for Mock {
    fn get_capabilities(&self) -> Result<TimeAlarmDeviceCapabilities, Self::Error> {
        Ok(TimeAlarmDeviceCapabilities(0xF7))
    }

    fn get_real_time(&self) -> Result<AcpiTimestamp, Self::Error> {
        Ok(self.rtc.time)
    }

    fn get_wake_status(&self, timer_id: AcpiTimerId) -> Result<TimerStatus, Self::Error> {
        Ok(self.rtc.get_timer(timer_id).timer_status)
    }

    fn get_expired_timer_wake_policy(&self, timer_id: AcpiTimerId) -> Result<AlarmExpiredWakePolicy, Self::Error> {
        Ok(self.rtc.get_timer(timer_id).wake_policy)
    }

    fn get_timer_value(&self, timer_id: AcpiTimerId) -> Result<AlarmTimerSeconds, Self::Error> {
        Ok(self.rtc.get_timer(timer_id).value)
    }
}
