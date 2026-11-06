//! Circular cover with a radial rendering of the engine's actual spectrum bands.
use iced::widget::{
    canvas::{self, Frame, Geometry, Path, Stroke},
    container, stack,
};
use iced::{Color, Length, Point, Rectangle};
use iced_adaptive_design::{self as iad, Element};

pub fn disc<'a, M: 'a>(id: usize, size: f32, bands: &'a [f32], visualizer: bool) -> Element<'a, M> {
    let art = iad::prelude::avatar(size * 0.66, "")
        .icon(iad::Icon::MusicNote)
        .shape(iad::prelude::AvatarShape::Circle)
        .color(iad::color::from_index(id));
    stack![
        canvas::Canvas::new(Ring { bands, visualizer })
            .width(size)
            .height(size),
        container(art).center(Length::Fixed(size)),
    ]
    .width(size)
    .height(size)
    .into()
}

struct Ring<'a> {
    bands: &'a [f32],
    visualizer: bool,
}

impl<M> canvas::Program<M, iad::Theme> for Ring<'_> {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        theme: &iad::Theme,
        bounds: Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let center = Point::new(bounds.width / 2.0, bounds.height / 2.0);
        let size = bounds.width.min(bounds.height);
        let color = theme.semantic.accent.bg;
        for (radius, opacity) in [(0.48, 0.025), (0.43, 0.04), (0.38, 0.07)] {
            frame.fill(
                &Path::circle(center, size * radius),
                Color {
                    a: opacity,
                    ..color
                },
            );
        }
        frame.stroke(
            &Path::circle(center, size * 0.35),
            Stroke::default()
                .with_width(1.0)
                .with_color(Color { a: 0.35, ..color }),
        );
        if self.visualizer {
            for index in 0..96 {
                let level = if self.bands.is_empty() {
                    0.0
                } else {
                    self.bands[index % self.bands.len()].clamp(0.0, 1.0)
                };
                let angle = std::f32::consts::TAU * index as f32 / 96.0;
                let inner = size * 0.37;
                let outer = inner + size * (0.008 + level * 0.105);
                let point = |radius| {
                    Point::new(
                        center.x + angle.cos() * radius,
                        center.y + angle.sin() * radius,
                    )
                };
                frame.stroke(
                    &Path::line(point(inner), point(outer)),
                    Stroke::default().with_width(2.0).with_color(Color {
                        a: 0.45 + level * 0.55,
                        ..color
                    }),
                );
            }
        }
        vec![frame.into_geometry()]
    }
}
