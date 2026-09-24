pub use iced::font::{Font, Style, Weight};

pub const TITLE: f32 = 24.0;
pub const NORMAL: f32 = 16.0;
pub const SMALL: f32 = 14.0;
pub const TINY: f32 = 12.0;

pub const BOLD: Font = Font {
    weight: Weight::Bold,
    ..Font::DEFAULT
};
