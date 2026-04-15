use crossterm::event::Event;
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Stylize, palette::tailwind},
    widgets::{Block, Paragraph, Widget},
};

use crate::app::Module;

const LABEL_COLOR: Color = tailwind::SLATE.c400;

#[derive(Default)]
pub struct Ucsi {}

impl Module for Ucsi {
    fn title(&self) -> &'static str {
        "UCSI Information"
    }

    fn update(&mut self) {}

    fn handle_event(&mut self, _evt: &Event) {}

    fn render(&self, area: Rect, buf: &mut Buffer) {
        Paragraph::new("Not yet implemented")
            .alignment(Alignment::Center)
            .italic()
            .fg(LABEL_COLOR)
            .block(Block::default())
            .render(area, buf);
    }
}

impl Ucsi {
    pub fn new() -> Self {
        Self {}
    }
}
