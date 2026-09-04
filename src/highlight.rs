use iced::widget::{self, text};
use iced::{Code, Font, Theme};

use std::ops::Range;
use std::path::Path;

/// The highlight token of `path`: the extension of the file, which
/// picks the grammar the syntax highlighter uses.
pub(crate) fn token(path: impl AsRef<Path>) -> String {
    path.as_ref()
        .extension()
        .map_or(String::new(), |extension| {
            extension.to_string_lossy().into_owned()
        })
}

/// A span of a region of `line` styled by the theme for `scope`.
pub(crate) fn span(line: &str, range: Range<usize>, code: Code) -> text::Span<'static> {
    let style = code.highlight(&Theme::CatppuccinMocha);

    widget::span(line[range].to_owned())
        .color_maybe(style.color)
        .font_maybe(style.style.map(|style| Font {
            style,
            ..Font::MONOSPACE
        }))
}
