use crate::app::Module;
use crate::common;
use crate::widgets::battery;
use battery_service_messages::{
    BatteryState, BatterySwapCapability, BatteryTechnology, BixFixedStrings, BstReturn, PowerUnit,
};
use core::ffi::CStr;
use ec_test_lib::BatterySource;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ratatui::style::Modifier;
use ratatui::text::Text;
use ratatui::widgets::{Row, StatefulWidget, Table, Widget};
use ratatui::{
    buffer::Buffer,
    crossterm::event::{Event, KeyCode, KeyEventKind},
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize, palette::tailwind},
    text::{Line, Span},
    widgets::{Block, Gauge, Paragraph},
};
use tui_input::{Input, backend::crossterm::EventHandler};

const BATGAUGE_COLOR_HIGH: Color = tailwind::GREEN.c400;
const BATGAUGE_COLOR_MEDIUM: Color = tailwind::AMBER.c400;
const BATGAUGE_COLOR_LOW: Color = tailwind::RED.c400;
const LABEL_COLOR: Color = tailwind::SKY.c300;
const MAX_SAMPLES: usize = 60;

fn str_from_bytes(bytes: &[u8]) -> String {
    CStr::from_bytes_until_nul(bytes)
        .ok()
        .and_then(|c| c.to_str().ok())
        .unwrap_or("Invalid")
        .to_owned()
}

#[cfg(test)]
fn charge_state_as_str(state: BatteryState) -> &'static str {
    if state.contains(BatteryState::DISCHARGING) {
        "Discharging"
    } else {
        "Charging"
    }
}

fn power_unit_as_capacity_str(power_unit: PowerUnit) -> &'static str {
    match power_unit {
        PowerUnit::MilliWatts => "mWh",
        PowerUnit::MilliAmps => "mAh",
    }
}

fn power_unit_as_rate_str(power_unit: PowerUnit) -> &'static str {
    match power_unit {
        PowerUnit::MilliWatts => "mW",
        PowerUnit::MilliAmps => "mA",
    }
}

fn bat_tech_as_str(bat_tech: BatteryTechnology) -> &'static str {
    match bat_tech {
        BatteryTechnology::Primary => "Primary",
        BatteryTechnology::Secondary => "Secondary",
    }
}

fn swap_cap_as_str(swap_cap: BatterySwapCapability) -> &'static str {
    match swap_cap {
        BatterySwapCapability::NonSwappable => "Non swappable",
        BatterySwapCapability::ColdSwappable => "Cold swappable",
        BatterySwapCapability::HotSwappable => "Hot swappable",
    }
}

/// All polled battery data in one place. Fields are public within the crate
/// so tests can inspect state without going through the full module machinery.
#[derive(Default)]
struct BatteryData {
    pub bst: BstReturn,
    pub bst_success: bool,
    pub bix: BixFixedStrings,
    pub bix_success: bool,
    pub samples: common::SampleBuf<u32, MAX_SAMPLES>,
    pub t_min: usize,
}

/// Fetch the latest BST reading into `data`. Isolated so it can be called
/// directly in unit tests without constructing a full `Battery<S>`.
fn poll_bst(data: &mut BatteryData, source: &impl BatterySource) {
    match source.get_bst() {
        Ok(bst) => {
            data.bst = bst;
            data.bst_success = true;
        }
        Err(_) => data.bst_success = false,
    }
}

/// Fetch static BIX info into `data` (called once at construction).
fn poll_bix(data: &mut BatteryData, source: &impl BatterySource) {
    match source.get_bix() {
        Ok(bix) => {
            data.bix = bix;
            data.bix_success = true;
        }
        Err(_) => data.bix_success = false,
    }
}

pub struct Battery<S: BatterySource> {
    data: BatteryData,
    /// Last trippoint set by the user.
    btp: u32,
    btp_success: bool,
    btp_input: Input,
    source: Arc<S>,
    /// How often to push a new point onto the capacity graph.
    graph_sample_interval: Duration,
    /// When the last graph sample was taken; `None` means "take one immediately".
    last_graph_update: Option<Instant>,
}

impl<S: BatterySource> Module for Battery<S> {
    fn title(&self) -> &'static str {
        "Battery Information"
    }

    fn update(&mut self) {
        poll_bst(&mut self.data, self.source.as_ref());

        let now = Instant::now();
        let update_graph = self
            .last_graph_update
            .is_none_or(|t| now.duration_since(t) >= self.graph_sample_interval);

        if update_graph {
            self.last_graph_update = Some(now);
            self.data.samples.insert(self.data.bst.battery_remaining_capacity);
            self.data.t_min += 1;
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        use Constraint::{Min, Percentage};
        // Top strip: live status bar + battery visual
        // Bottom: BIX static info (left) | capacity chart (right)
        let [strip_area, bottom_area] =
            Layout::vertical([Percentage(22), Min(0)]).areas(area);
        let [bix_area, chart_area] =
            Layout::horizontal([Percentage(50), Percentage(50)]).areas(bottom_area);

        self.render_status_strip(strip_area, buf);
        self.render_bix(bix_area, buf);
        self.render_bst_chart(chart_area, buf);
    }

    fn handle_event(&mut self, evt: &Event) {
        if let Event::Key(key) = evt
            && key.code == KeyCode::Enter
            && key.kind == KeyEventKind::Press
        {
            if let Ok(btp) = self.btp_input.value_and_reset().parse() {
                if self.source.set_btp(btp).is_ok() {
                    self.btp = btp;
                    self.btp_success = true;
                } else {
                    self.btp_success = false;
                }
            }
        } else {
            let _ = self.btp_input.handle_event(evt);
        }
    }

    fn render_card(&self, area: Rect, buf: &mut Buffer) {
        let is_charging = self.data.bst.battery_state.contains(BatteryState::CHARGING);
        let state_str = if is_charging { "▲ Charging" } else { "▼ Discharging" };
        let state_color = if is_charging { tailwind::GREEN.c400 } else { tailwind::AMBER.c400 };

        let bat_pct = bat_percent(self.data.bst.battery_remaining_capacity, self.data.bix.design_capacity);
        let rate = self.data.bst.battery_present_rate;
        let cap_str = power_unit_as_capacity_str(self.data.bix.power_unit);
        let rate_str = power_unit_as_rate_str(self.data.bix.power_unit);

        let block = Block::bordered()
            .title(common::status_title("Battery", self.data.bst_success))
            .border_style(tailwind::SKY.c700);
        let inner = block.inner(area);
        block.render(area, buf);

        let [state_area, gauge_area, details_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(0)])
                .areas(inner);

        Span::styled(state_str, Style::default().fg(state_color).bold())
            .render(state_area, buf);

        let gauge_color = bat_gauge_color(
            self.data.bst.battery_remaining_capacity,
            self.data.bix.design_cap_of_warning,
            self.data.bix.design_cap_of_low,
        );
        Gauge::default()
            .gauge_style(gauge_color)
            .percent(bat_pct)
            .render(gauge_area, buf);

        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("Remaining  ", Style::default().fg(LABEL_COLOR).bold()),
                Span::raw(format!(
                    "{} / {} {}",
                    self.data.bst.battery_remaining_capacity,
                    self.data.bix.design_capacity,
                    cap_str
                )),
            ]),
            Line::from(vec![
                Span::styled("Rate       ", Style::default().fg(LABEL_COLOR).bold()),
                Span::raw(format!("{rate} {rate_str}")),
            ]),
        ])
        .render(details_area, buf);
    }
}

fn bat_percent(remaining: u32, design: u32) -> u16 {
    (remaining * 100).checked_div(design).unwrap_or(0).clamp(0, 100) as u16
}

fn bat_gauge_color(remaining: u32, warning: u32, low: u32) -> Color {
    if remaining <= low {
        BATGAUGE_COLOR_LOW
    } else if remaining <= warning {
        BATGAUGE_COLOR_MEDIUM
    } else {
        BATGAUGE_COLOR_HIGH
    }
}

impl<S: BatterySource> Battery<S> {
    pub fn new(source: Arc<S>) -> Self {
        let mut inst = Self {
            data: Default::default(),
            btp: 0,
            btp_success: true,
            btp_input: Input::default(),
            source,
            graph_sample_interval: Duration::from_secs(1),
            last_graph_update: None,
        };

        // BIX info is static — read once at construction.
        poll_bix(&mut inst.data, inst.source.as_ref());

        inst.update();
        inst
    }

    /// Set how often the capacity graph is updated. Defaults to 1 second.
    pub fn with_graph_sample_interval(self, interval: Duration) -> Self {
        Self {
            graph_sample_interval: interval,
            ..self
        }
    }

    fn render_status_strip(&self, area: Rect, buf: &mut Buffer) {
        use Constraint::{Length, Min};
        let is_charging = self.data.bst.battery_state.contains(BatteryState::CHARGING);
        let state_str = if is_charging { "▲ Charging" } else { "▼ Discharging" };
        let state_color = if is_charging { tailwind::GREEN.c400 } else { tailwind::AMBER.c400 };
        let pct = bat_percent(self.data.bst.battery_remaining_capacity, self.data.bix.design_capacity);
        let cap_str = power_unit_as_capacity_str(self.data.bix.power_unit);
        let rate_str = power_unit_as_rate_str(self.data.bix.power_unit);

        let block = Block::bordered()
            .title(
                Line::from(vec![
                    Span::styled(state_str, Style::default().fg(state_color).bold()),
                    Span::styled(
                        format!("  {pct}%"),
                        Style::default().fg(Color::White).bold(),
                    ),
                ])
            )
            .border_style(tailwind::SKY.c600);
        let inner = block.inner(area);
        block.render(area, buf);

        // Split inner: gauge fills most of width, battery visual on the right
        let [left_area, bat_area] = Layout::horizontal([Min(0), Length(6)]).areas(inner);
        let [gauge_area, details_area] = Layout::vertical([Length(1), Min(0)]).areas(left_area);

        let gauge_color = bat_gauge_color(
            self.data.bst.battery_remaining_capacity,
            self.data.bix.design_cap_of_warning,
            self.data.bix.design_cap_of_low,
        );
        Gauge::default()
            .gauge_style(gauge_color)
            .percent(pct)
            .render(gauge_area, buf);

        Paragraph::new(vec![
            common::metric_row(
                "Remaining ",
                format!(
                    "{} / {} {}",
                    self.data.bst.battery_remaining_capacity,
                    self.data.bix.design_capacity,
                    cap_str
                ),
                LABEL_COLOR,
            ),
            common::metric_row(
                "Rate      ",
                format!("{} {}", self.data.bst.battery_present_rate, rate_str),
                LABEL_COLOR,
            ),
            common::metric_row(
                "Voltage   ",
                format!("{} mV", self.data.bst.battery_present_voltage),
                LABEL_COLOR,
            ),
        ])
        .render(details_area, buf);

        // Compact battery visual
        let mut bat_state = battery::BatteryState::new(
            self.data.bst.battery_remaining_capacity,
            is_charging,
        );
        battery::Battery::default()
            .color_high(BATGAUGE_COLOR_HIGH)
            .color_warning(BATGAUGE_COLOR_MEDIUM)
            .color_low(BATGAUGE_COLOR_LOW)
            .design_capacity(self.data.bix.design_capacity)
            .warning_capacity(self.data.bix.design_cap_of_warning)
            .low_capacity(self.data.bix.design_cap_of_low)
            .render(bat_area, buf, &mut bat_state);
    }

    fn render_bst_chart(&self, area: Rect, buf: &mut Buffer) {
        let y_labels = [
            "0".bold(),
            Span::styled(
                format!("{}", self.data.bix.design_capacity / 2),
                Style::default().bold(),
            ),
            Span::styled(format!("{}", self.data.bix.design_capacity), Style::default().bold()),
        ];
        let graph = common::Graph {
            title: "Capacity vs Time".to_string(),
            color: tailwind::SKY.c400,
            samples: self.data.samples.get(),
            x_axis: "Time (m)".to_string(),
            x_bounds: [0.0, 60.0],
            x_labels: common::time_labels(self.data.t_min, MAX_SAMPLES),
            y_axis: format!("Capacity ({})", power_unit_as_capacity_str(self.data.bix.power_unit)),
            y_bounds: [0.0, self.data.bix.design_capacity as f64],
            y_labels,
        };
        common::render_chart(area, buf, graph);
    }

    fn bix_rows(&self) -> Vec<Row<'static>> {
        let power_unit = self.data.bix.power_unit;
        let cap = power_unit_as_capacity_str(power_unit);
        let rate = power_unit_as_rate_str(power_unit);

        vec![
            Row::new(vec![
                Text::raw("Revision").add_modifier(Modifier::BOLD),
                format!("{}", self.data.bix.revision).into(),
            ]),
            Row::new(vec![
                Text::raw("Power Unit").add_modifier(Modifier::BOLD),
                rate.into(),
            ]),
            Row::new(vec![
                Text::raw("Design Capacity").add_modifier(Modifier::BOLD),
                format!("{} {}", self.data.bix.design_capacity, cap).into(),
            ]),
            Row::new(vec![
                Text::raw("Last Full Capacity").add_modifier(Modifier::BOLD),
                format!("{} {}", self.data.bix.last_full_charge_capacity, cap).into(),
            ]),
            Row::new(vec![
                Text::raw("Technology").add_modifier(Modifier::BOLD),
                bat_tech_as_str(self.data.bix.battery_technology).into(),
            ]),
            Row::new(vec![
                Text::raw("Design Voltage").add_modifier(Modifier::BOLD),
                format!("{} mV", self.data.bix.design_voltage).into(),
            ]),
            Row::new(vec![
                Text::raw("Warning Capacity").add_modifier(Modifier::BOLD),
                format!("{} {}", self.data.bix.design_cap_of_warning, cap).into(),
            ]),
            Row::new(vec![
                Text::raw("Low Capacity").add_modifier(Modifier::BOLD),
                format!("{} {}", self.data.bix.design_cap_of_low, cap).into(),
            ]),
            Row::new(vec![
                Text::raw("Cycle Count").add_modifier(Modifier::BOLD),
                format!("{}", self.data.bix.cycle_count).into(),
            ]),
            Row::new(vec![
                Text::raw("Accuracy").add_modifier(Modifier::BOLD),
                format!("{}%", self.data.bix.measurement_accuracy as f64 / 1000.0).into(),
            ]),
            Row::new(vec![
                Text::raw("Max Sample Time").add_modifier(Modifier::BOLD),
                format!("{} ms", self.data.bix.max_sampling_time).into(),
            ]),
            Row::new(vec![
                Text::raw("Min Sample Time").add_modifier(Modifier::BOLD),
                format!("{} ms", self.data.bix.min_sampling_time).into(),
            ]),
            Row::new(vec![
                Text::raw("Max Avg Interval").add_modifier(Modifier::BOLD),
                format!("{} ms", self.data.bix.max_averaging_interval).into(),
            ]),
            Row::new(vec![
                Text::raw("Min Avg Interval").add_modifier(Modifier::BOLD),
                format!("{} ms", self.data.bix.min_averaging_interval).into(),
            ]),
            Row::new(vec![
                Text::raw("Cap. Granularity 1").add_modifier(Modifier::BOLD),
                format!("{} {}", self.data.bix.battery_capacity_granularity_1, cap).into(),
            ]),
            Row::new(vec![
                Text::raw("Cap. Granularity 2").add_modifier(Modifier::BOLD),
                format!("{} {}", self.data.bix.battery_capacity_granularity_2, cap).into(),
            ]),
            Row::new(vec![
                Text::raw("Model Number").add_modifier(Modifier::BOLD),
                str_from_bytes(&self.data.bix.model_number).into(),
            ]),
            Row::new(vec![
                Text::raw("Serial Number").add_modifier(Modifier::BOLD),
                str_from_bytes(&self.data.bix.serial_number).into(),
            ]),
            Row::new(vec![
                Text::raw("Battery Type").add_modifier(Modifier::BOLD),
                str_from_bytes(&self.data.bix.battery_type).into(),
            ]),
            Row::new(vec![
                Text::raw("OEM Info").add_modifier(Modifier::BOLD),
                str_from_bytes(&self.data.bix.oem_info).into(),
            ]),
            Row::new(vec![
                Text::raw("Swap Capability").add_modifier(Modifier::BOLD),
                swap_cap_as_str(self.data.bix.battery_swapping_capability).into(),
            ]),
        ]
    }

    fn render_bix(&self, area: Rect, buf: &mut Buffer) {
        use Constraint::{Length, Min};
        // Reserve bottom rows for BTP input
        let btp_height = 3u16;
        let [table_area, btp_area] =
            Layout::vertical([Min(0), Length(btp_height)]).areas(area);

        let table = Table::new(self.bix_rows(), [Constraint::Min(22), Constraint::Fill(1)])
            .block(
                Block::bordered()
                    .title(common::status_title("Battery Info (BIX)", self.data.bix_success))
                    .fg(LABEL_COLOR),
            )
            .style(Style::new().white());
        Widget::render(table, table_area, buf);

        self.render_btp_input(btp_area, buf);
    }

    fn render_btp_input(&self, area: Rect, buf: &mut Buffer) {
        let width = area.width.max(3) - 3;
        let scroll = self.btp_input.visual_scroll(width as usize);
        let title = format!(
            "Trippoint: {} {}  <set new value + Enter>",
            self.btp,
            power_unit_as_capacity_str(self.data.bix.power_unit)
        );
        let dot = if self.btp_success {
            Span::styled("● ", Style::default().fg(tailwind::GREEN.c400))
        } else {
            Span::styled("● ", Style::default().fg(tailwind::RED.c500))
        };
        let block_title = Line::from(vec![dot, Span::raw(title)]);

        Paragraph::new(self.btp_input.value())
            .scroll((0, scroll as u16))
            .block(
                Block::bordered()
                    .title(block_title)
                    .border_style(Style::default().fg(tailwind::SKY.c700)),
            )
            .render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::test_support::TestError;
    use battery_service_messages::{BatterySwapCapability, BatteryTechnology, PowerUnit};
    use ec_test_lib::{BatterySource, ErrorType};

    // ── test doubles ─────────────────────────────────────────────────────────

    struct OkSource;
    impl ErrorType for OkSource {
        type Error = TestError;
    }
    impl BatterySource for OkSource {
        fn get_bst(&self) -> Result<BstReturn, Self::Error> {
            Ok(BstReturn {
                battery_state: BatteryState::CHARGING,
                battery_present_rate: 1000,
                battery_remaining_capacity: 5000,
                battery_present_voltage: 12000,
            })
        }
        fn get_bix(&self) -> Result<BixFixedStrings, Self::Error> {
            Ok(BixFixedStrings {
                design_capacity: 10000,
                cycle_count: 42,
                power_unit: PowerUnit::MilliWatts,
                ..Default::default()
            })
        }
        fn set_btp(&self, _: u32) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    struct ErrSource;
    impl ErrorType for ErrSource {
        type Error = TestError;
    }
    impl BatterySource for ErrSource {
        fn get_bst(&self) -> Result<BstReturn, Self::Error> {
            Err(TestError)
        }
        fn get_bix(&self) -> Result<BixFixedStrings, Self::Error> {
            Err(TestError)
        }
        fn set_btp(&self, _: u32) -> Result<(), Self::Error> {
            Err(TestError)
        }
    }

    // ── poll_bst ─────────────────────────────────────────────────────────────

    #[test]
    fn poll_bst_sets_success_flag_on_ok() {
        let mut data = BatteryData::default();
        poll_bst(&mut data, &OkSource);
        assert!(data.bst_success);
        assert_eq!(data.bst.battery_remaining_capacity, 5000);
    }

    #[test]
    fn poll_bst_clears_success_flag_on_err() {
        let mut data = BatteryData::default();
        // Seed with a known value so we can confirm it is not overwritten.
        data.bst.battery_remaining_capacity = 99;
        poll_bst(&mut data, &ErrSource);
        assert!(!data.bst_success);
        // The stale value should remain unchanged on failure.
        assert_eq!(data.bst.battery_remaining_capacity, 99);
    }

    // ── poll_bix ─────────────────────────────────────────────────────────────

    #[test]
    fn poll_bix_sets_success_flag_on_ok() {
        let mut data = BatteryData::default();
        poll_bix(&mut data, &OkSource);
        assert!(data.bix_success);
        assert_eq!(data.bix.design_capacity, 10000);
        assert_eq!(data.bix.cycle_count, 42);
    }

    #[test]
    fn poll_bix_clears_success_flag_on_err() {
        let mut data = BatteryData::default();
        data.bix.design_capacity = 9999;
        poll_bix(&mut data, &ErrSource);
        assert!(!data.bix_success);
        assert_eq!(data.bix.design_capacity, 9999);
    }

    // ── format helpers ───────────────────────────────────────────────────────

    #[test]
    fn charge_state_discharging() {
        assert_eq!(charge_state_as_str(BatteryState::DISCHARGING), "Discharging");
    }

    #[test]
    fn charge_state_charging() {
        assert_eq!(charge_state_as_str(BatteryState::CHARGING), "Charging");
    }

    #[test]
    fn str_from_bytes_valid_nul_terminated() {
        let bytes = b"Li-ion\0\0";
        assert_eq!(str_from_bytes(bytes), "Li-ion");
    }

    #[test]
    fn str_from_bytes_no_nul_returns_invalid() {
        let bytes = b"no nul here";
        assert_eq!(str_from_bytes(bytes), "Invalid");
    }

    #[test]
    fn power_unit_capacity_strings() {
        assert_eq!(power_unit_as_capacity_str(PowerUnit::MilliWatts), "mWh");
        assert_eq!(power_unit_as_capacity_str(PowerUnit::MilliAmps), "mAh");
    }

    #[test]
    fn power_unit_rate_strings() {
        assert_eq!(power_unit_as_rate_str(PowerUnit::MilliWatts), "mW");
        assert_eq!(power_unit_as_rate_str(PowerUnit::MilliAmps), "mA");
    }

    #[test]
    fn bat_tech_strings() {
        assert_eq!(bat_tech_as_str(BatteryTechnology::Primary), "Primary");
        assert_eq!(bat_tech_as_str(BatteryTechnology::Secondary), "Secondary");
    }

    #[test]
    fn swap_cap_strings() {
        assert_eq!(swap_cap_as_str(BatterySwapCapability::NonSwappable), "Non swappable");
        assert_eq!(swap_cap_as_str(BatterySwapCapability::ColdSwappable), "Cold swappable");
        assert_eq!(swap_cap_as_str(BatterySwapCapability::HotSwappable), "Hot swappable");
    }
}
