use iced::highlighter::{Highlight, Scope};
use iced::widget::{self, text};
use iced::{Font, Theme};

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
pub(crate) fn span(line: &str, range: Range<usize>, scope: Scope) -> text::Span<'static> {
    let format = Theme::CatppuccinMocha.highlight(scope);

    widget::span(line[range].to_owned())
        .color_maybe(format.color)
        .font_maybe(format.style.map(|style| Font {
            style,
            ..Font::MONOSPACE
        }))
}
