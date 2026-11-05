//! Listening analytics for the trial application. The current library has synthetic,
//! lifetime play counters rather than timestamped play events, so all period-scoped values on
//! this page are explicitly illustrative until the engine exposes playback history.

use iced::widget::{button, column, container, row, scrollable, svg, text, text_input};
use iced::Length;
use iced_adaptive_design as iad;
use iad::prelude::typography as ty;

use crate::{App, Element, Message};

fn chart_svg(kind: &str, values: &[f32], labels: &[&str]) -> String {
    let colors = ["#3584e4", "#33d17a", "#e5a50a", "#c061cb", "#f66151", "#62a0ea"];
    let total = values.iter().sum::<f32>().max(1.0);
    let mut start = -std::f32::consts::FRAC_PI_2;
    let mut paths = String::new();
    for (i, value) in values.iter().enumerate() {
        let sweep = value.max(0.0) / total * std::f32::consts::TAU;
        let end = start + sweep;
        let (cx, cy, r) = if kind == "sunburst" { (150.0, 120.0, 78.0) } else { (100.0, 100.0, 70.0) };
        let x1 = cx + r * start.cos();
        let y1 = cy + r * start.sin();
        let x2 = cx + r * end.cos();
        let y2 = cy + r * end.sin();
        let large = if sweep > std::f32::consts::PI { 1 } else { 0 };
        paths.push_str(&format!(
            r##"<path d="M {cx:.2} {cy:.2} L {x1:.2} {y1:.2} A {r} {r} 0 {large} 1 {x2:.2} {y2:.2} Z" fill="{}" stroke="#242424" stroke-width="1"/>"##,
            colors[i % colors.len()]
        ));
        start = end;
    }
    let (width, height, viewbox) = if kind == "sunburst" { (300, 240, "0 0 300 240") } else { (200, 200, "0 0 200 200") };
    let legend = labels.iter().enumerate().map(|(i, label)| {
        format!(r#"<rect x="{}" y="{}" width="9" height="9" rx="2" fill="{}"/><text x="{}" y="{}" font-size="10" fill="currentColor">{}</text>"#,
            if kind == "sunburst" { 8 } else { 0 }, 8 + i * 17, colors[i % colors.len()],
            if kind == "sunburst" { 22 } else { 14 }, 17 + i * 17, label)
    }).collect::<String>();
    format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="{viewbox}">{paths}{legend}</svg>"##)
}

fn line_svg(days: usize) -> String {
    let values: Vec<f32> = (0..days.min(30)).map(|i| {
        let x = i as f32;
        2.0 + ((x * 1.7).sin() + 1.0) * 1.3 + (x % 5.0) * 0.12
    }).collect();
    let max = values.iter().copied().fold(1.0_f32, f32::max);
    let points = values.iter().enumerate().map(|(i, v)| {
        let x = 12.0 + i as f32 * 276.0 / (values.len().saturating_sub(1).max(1) as f32);
        let y = 90.0 - v / max * 70.0;
        format!("{x:.1},{y:.1}")
    }).collect::<Vec<_>>().join(" ");
    format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="300" height="112" viewBox="0 0 300 112"><path d="M 12 92 H 288" stroke="currentColor" opacity=".2"/><polyline points="{points}" fill="none" stroke="#3584e4" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/></svg>"##)
}

fn chart(svg_text: String, height: f32) -> Element<'static> {
    container(svg(svg::Handle::from_memory(svg_text.into_bytes())))
        .width(Length::Fill)
        .height(Length::Fixed(height))
        .center_x(Length::Fill)
        .into()
}

impl App {
    pub fn about_me(&self) -> Element<'_> {
        let days = self.stats_days.max(1) as u32;
        let active: Vec<_> = self.lib.tracks.iter()
            .filter(|t| t.last_played_days < days)
            .collect();
        let plays: u32 = active.iter().map(|t| t.plays.min(20)).sum();
        let listened_hours = plays as f32 * 3.6 / 60.0;
        let top_artist = self.lib.most_listened_artists(1).first()
            .map(|(i, _)| self.lib.artists()[*i].0.clone()).unwrap_or_else(|| "No data".into());
        let top_track = self.lib.most_listened_tracks(1).first()
            .map(|t| format!("{} · {}", t.title, t.artist)).unwrap_or_else(|| "No data".into());
        let range = row![
            button(text("24 hours")).on_press(Message::StatsDays(1)),
            button(text("1 week")).on_press(Message::StatsDays(7)),
            button(text("1 month")).on_press(Message::StatsDays(30)),
            button(text("1 year")).on_press(Message::StatsDays(365)),
        ].spacing(6).wrap();
        let custom = row![
            text_input("Custom days", &self.stats_custom)
                .on_input(Message::StatsCustom)
                .width(Length::Fixed(130.0)),
            ty::caption(format!("Showing last {days} days")),
        ].spacing(8).align_y(iced::Alignment::Center);
        let metrics = iad::prelude::surface::band(row![
            column![ty::title_2(format!("{listened_hours:.1}h")), ty::caption("Listening time")].width(Length::Fill),
            column![ty::title_2(format!("{}", plays)), ty::caption("Tracks played")].width(Length::Fill),
            column![ty::title_2(top_artist), ty::caption("Top artist")].width(Length::Fill),
        ].spacing(12).wrap());
        let artist_values = self.lib.most_listened_artists(5).iter().map(|(_, n)| *n as f32).collect::<Vec<_>>();
        let artist_labels = self.lib.most_listened_artists(5).iter()
            .map(|(i, _)| self.lib.artists()[*i].0.as_str()).collect::<Vec<_>>();
        let genre_labels = ["Bachata", "Zouk", "Salsa", "Other"];
        let genre_values = [34.0, 28.0, 16.0, 22.0];
        let track_count = active.len();
        let daily = chart(line_svg(days.min(30) as usize), 118.0);
        let artists = chart(chart_svg("pie", &artist_values, &artist_labels), 210.0);
        let genres = chart(chart_svg("sunburst", &genre_values, &genre_labels), 240.0);
        let tags = self.lib.tag_names().into_iter().take(8).map(|tag| {
            container(ty::caption(tag)).padding([5, 9]).style(iad::styles::container::card)
        }).collect::<Vec<_>>();
        let page = container(column![
            ty::title_2("About me"),
            ty::caption("Your listening habits, taste and discoveries"),
            range,
            custom,
            metrics,
            ty::caption(format!("{track_count} distinct tracks in this sample period")),
            column![
                ty::heading("Listening activity"),
                daily,
                ty::caption("Daily listening time · illustrative sample")
            ].spacing(8),
            row![
                column![ty::heading("Genres"), genres].width(Length::Fill),
                column![ty::heading("Artists"), artists].width(Length::Fill),
            ].spacing(12).wrap(),
            column![
                ty::heading("Favourite track"),
                ty::body(top_track),
                ty::heading("Favourite tags"),
                row(tags).spacing(6).wrap(),
            ].spacing(8),
            column![
                ty::heading("Listening behaviour"),
                ty::caption("Next: completion rate, skips, repeats, session length, album completion and rediscovery.")
            ].spacing(6),
            ty::caption("Demo analytics: the current GUI fixture has lifetime play counts and last-played age, not timestamped playback events. Period totals and charts are illustrative; real period-accurate analytics require engine playback events.")
        ].spacing(16)).padding(12);
        scrollable(page).height(Length::Fill).into()
    }
}
