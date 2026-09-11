use crate::font;
use crate::locale;

use iced::widget::{container, scrollable, text};
use iced::{Element, Theme};

pub fn snaps(viewport: scrollable::Viewport) -> bool {
    const MAX_DISTANCE: f32 = 20.0;

    let offset = viewport.absolute_offset();
    let bounds = viewport.bounds();
    let content_bounds = viewport.content_bounds();

    let distance_to_bottom = (content_bounds.height - bounds.height - offset.y).max(0.0);

    distance_to_bottom <= MAX_DISTANCE
}

pub fn context_led<'a, Message: 'a>(
    context_size: Option<u64>,
    timings: Option<reason::Timings>,
) -> Element<'a, Message> {
    use iced::mouse;
    use iced::widget::{canvas, tooltip};
    use iced::{Radians, Rectangle, Renderer};

    use std::cell::RefCell;
    use std::f32::consts::{FRAC_PI_2, PI};

    const SIZE: f32 = 14.0;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    struct Led {
        context_size: Option<u64>,
        timings: Option<reason::Timings>,
    }

    #[derive(Default)]
    struct State {
        last: RefCell<Led>,
        cache: canvas::Cache,
    }

    impl<Message> canvas::Program<Message> for Led {
        type State = State;

        fn draw(
            &self,
            state: &Self::State,
            renderer: &Renderer,
            theme: &Theme,
            bounds: Rectangle,
            _cursor: mouse::Cursor,
        ) -> Vec<canvas::Geometry> {
            const STROKE_WIDTH: f32 = 2.0;

            if *state.last.borrow() != *self {
                *state.last.borrow_mut() = *self;
                state.cache.clear();
            }

            let geometry = state.cache.draw(renderer, bounds.size(), |frame| {
                let palette = theme.palette();
                let radius = (frame.width() - STROKE_WIDTH) / 2.0;
                let circle = canvas::Path::circle(frame.center(), radius);

                frame.stroke(
                    &circle,
                    canvas::Stroke {
                        style: canvas::Style::Solid(palette.background.strong.color),
                        width: STROKE_WIDTH,
                        ..canvas::Stroke::default()
                    },
                );

                if let Some(timings) = self.timings
                    && let Some(context_size) = self.context_size
                {
                    let usage = timings.total_tokens() as f32 / context_size as f32;

                    let arc = {
                        let mut builder = canvas::path::Builder::new();

                        let start = -FRAC_PI_2;

                        builder.arc(canvas::path::Arc {
                            center: frame.center(),
                            radius,
                            start_angle: Radians(start),
                            end_angle: Radians(start + 2.0 * PI * usage),
                        });

                        builder.build()
                    };

                    frame.stroke(
                        &arc,
                        canvas::Stroke {
                            style: canvas::Style::Solid(match usage {
                                0.0..0.8 => palette.primary.base.color,
                                0.8..0.9 => palette.warning.base.color,
                                _ => palette.danger.base.color,
                            }),
                            width: STROKE_WIDTH,
                            line_cap: canvas::LineCap::Square,
                            ..canvas::Stroke::default()
                        },
                    );
                }
            });

            vec![geometry]
        }
    }

    let led = canvas(Led {
        timings,
        context_size,
    })
    .width(SIZE)
    .height(SIZE);

    match (context_size, timings) {
        (Some(context_size), Some(timings)) => {
            let tokens = timings.total_tokens();
            let percent = tokens as f32 / context_size as f32 * 100.0;

            tooltip(
                led,
                text!(
                    "{} / {} ({percent:.1}%)",
                    locale::thousands(tokens),
                    locale::thousands(context_size)
                )
                .size(font::SMALL),
                tooltip::Position::Top,
            )
            .style(container::rounded_box)
            .into()
        }
        _ => led.into(),
    }
}
