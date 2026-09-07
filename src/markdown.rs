use iced::widget::markdown;
use iced::{Element, Font, Pixels, Theme};

pub use markdown::{Item, Uri};

#[derive(Debug, Default)]
pub struct Markdown {
    raw: String,
    content: markdown::Content,
}

impl Markdown {
    pub fn new(raw: String) -> Self {
        Self {
            content: markdown::Content::parse(&raw),
            raw,
        }
    }

    pub fn push_str(&mut self, delta: &str) {
        self.raw.push_str(delta);
        self.content.push_str(delta);
    }

    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn items(&self) -> &[Item] {
        self.content.items()
    }
}

pub fn view(items: &[Item], font: Font, size: impl Into<Pixels>) -> Element<'_, Uri> {
    markdown(
        items,
        markdown::Settings {
            font,
            ..markdown::Settings::with_text_size(size)
        }
        .line_height(1.5),
        Theme::CatppuccinMocha,
    )
}
