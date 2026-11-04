//! A list of media (tracks, episodes, videos): one layout for the sortable header and every row, so
//! the columns line up by construction. Generic, with nothing of weave in it, to upstream into
//! iced-adaptive-design. It replaces the `MediaColumns`, `MediaRow` and `SortHeader` that iad
//! dropped in favour of `ResponsiveTable`, and is where those came from: weave's own flat track
//! row and sort header, generalised.
//!
//! - [`MediaListLayout`] declares the columns once: the title (with its subtitle beneath it, and
//!   a picture before it), further columns with a width and an alignment, and whether rows end
//!   in a menu. It builds the [header](MediaListLayout::header) and the [rows](MediaListLayout::row).
//! - [`MediaListRow`] is one row: the whole row is a flat button (it lights up as one, menu
//!   included), and the trailing menu is laid over it, so it takes its own presses.
//!
//! In compact mode the picture goes and the subtitle becomes its own column, so a dense list
//! still shows the artist.

use iad::Icon;
use iad::primitives::sorting::{Direction, SortState};
use iad::widget::typography as ty;
use iced::alignment::Horizontal;
use iced::widget::{button, column, container, row, space, stack};
use iced::{Alignment, Length};
use iced_adaptive_design as iad;

type Element<'a, M> = iad::Element<'a, M>;

/// Space between columns.
const GAP: f32 = 12.0;
/// The side of the picture of a row.
const PICTURE: f32 = 40.0;
/// The space between a row's edge (where its highlight ends) and its content.
const INSET: f32 = 8.0;
/// The height of the header.
const HEADER_H: f32 = 32.0;
/// The width of the trailing menu's slot.
const MENU_W: f32 = 40.0;

#[derive(Clone)]
struct Slot<K> {
    /// What sorting by this column sorts by; `None` for a column that is not sortable.
    key: Option<K>,
    label: String,
    width: Length,
    align: Horizontal,
}

/// The columns of a media list, declared once for the header and the rows.
#[derive(Clone)]
pub struct MediaListLayout<K> {
    compact: bool,
    picture: bool,
    menu: bool,
    /// The title, then the subtitle when compact, then the other columns in order.
    slots: Vec<Slot<K>>,
}

impl<K: Clone + PartialEq + 'static> MediaListLayout<K> {
    /// `compact`: no picture, and the subtitle in a column of its own.
    pub fn new(compact: bool) -> Self {
        Self {
            compact,
            picture: !compact,
            menu: false,
            slots: Vec::new(),
        }
    }

    /// The main column: it takes the free width. Declare it first.
    pub fn title(mut self, key: K, label: impl Into<String>) -> Self {
        self.slots.insert(
            0,
            Slot {
                key: Some(key),
                label: label.into(),
                width: Length::Fill,
                align: Horizontal::Left,
            },
        );
        self
    }

    /// The line under the title, or a column of its own when compact. Declare it right after
    /// the title.
    pub fn subtitle(mut self, key: K, label: impl Into<String>) -> Self {
        if self.compact {
            self.slots.push(Slot {
                key: Some(key),
                label: label.into(),
                width: Length::Fill,
                align: Horizontal::Left,
            });
        }
        self
    }

    /// A sortable column.
    pub fn column(
        mut self,
        key: K,
        label: impl Into<String>,
        width: Length,
        align: Horizontal,
    ) -> Self {
        self.slots.push(Slot {
            key: Some(key),
            label: label.into(),
            width,
            align,
        });
        self
    }

    /// A column that cannot be sorted by.
    pub fn plain_column(
        mut self,
        label: impl Into<String>,
        width: Length,
        align: Horizontal,
    ) -> Self {
        self.slots.push(Slot {
            key: None,
            label: label.into(),
            width,
            align,
        });
        self
    }

    /// Rows end in a menu (a slot after the last column).
    pub fn menu(mut self, on: bool) -> Self {
        self.menu = on;
        self
    }

    /// The offset of the title's text from its column's edge: where the picture ends.
    fn title_inset(&self) -> f32 {
        if self.picture { PICTURE + GAP } else { 0.0 }
    }

    /// The header: the column labels, each a button that sorts by it, with an arrow on the
    /// column the list is sorted by.
    pub fn header<'a, M: Clone + 'a>(
        &self,
        sort: &SortState<K>,
        on_sort: impl Fn(K) -> M + 'a,
    ) -> Element<'a, M> {
        let inset = self.title_inset();
        let mut r = row![].spacing(GAP).align_y(Alignment::Center);
        for (i, s) in self.slots.iter().enumerate() {
            let arrow = s
                .key
                .as_ref()
                .and_then(|k| sort.direction(k))
                .map(|d| match d {
                    Direction::Asc => Icon::GoUp,
                    Direction::Desc => Icon::GoDown,
                });
            let mut label = row![].spacing(4).align_y(Alignment::Center);
            if i == 0 && inset > 0.0 {
                label = label.push(space::horizontal().width(inset));
            }
            label = label.push(ty::caption(s.label.clone()));
            if let Some(a) = arrow {
                label = label.push(a.view(12.0));
            }
            let cell: Element<'a, M> = container(label).width(Length::Fill).align_x(s.align).into();
            let slot: Element<'a, M> = match &s.key {
                Some(k) => button(cell)
                    .padding(0) // gutter-ok: the whole label is the button
                    .width(s.width)
                    .style(iad::style::button::flat)
                    .on_press(on_sort(k.clone()))
                    .into(),
                None => container(cell).width(s.width).into(),
            };
            r = r.push(slot);
        }
        if self.menu {
            r = r.push(space::horizontal().width(MENU_W));
        }
        container(r)
            .padding([0.0, INSET]) // gutter-ok: the rows' own inset, so the columns line up
            .height(HEADER_H)
            .center_y(HEADER_H)
            .into()
    }

    /// A row with this title.
    pub fn row<'a, M: Clone + 'a>(&self, title: impl Into<String>) -> MediaListRow<'a, K, M> {
        MediaListRow {
            layout: self.clone(),
            title: title.into(),
            subtitle: None,
            picture: None,
            cells: Vec::new(),
            menu: None,
            on_press: None,
            height: Length::Fill,
        }
    }
}

/// One row of a [`MediaListLayout`].
pub struct MediaListRow<'a, K, M> {
    layout: MediaListLayout<K>,
    title: String,
    subtitle: Option<String>,
    picture: Option<Element<'a, M>>,
    cells: Vec<Element<'a, M>>,
    menu: Option<Element<'a, M>>,
    on_press: Option<M>,
    height: Length,
}

impl<'a, K: Clone + PartialEq + 'static, M: Clone + 'a> MediaListRow<'a, K, M> {
    /// The line under the title (a column of its own when compact).
    pub fn subtitle(mut self, s: impl Into<String>) -> Self {
        self.subtitle = Some(s.into());
        self
    }

    /// The picture before the title; not shown when compact.
    pub fn picture(mut self, p: impl Into<Element<'a, M>>) -> Self {
        self.picture = Some(p.into());
        self
    }

    /// The next column's content, in the order the columns were declared.
    pub fn cell(mut self, c: impl Into<Element<'a, M>>) -> Self {
        self.cells.push(c.into());
        self
    }

    /// The trailing menu.
    pub fn menu(mut self, m: impl Into<Element<'a, M>>) -> Self {
        self.menu = Some(m.into());
        self
    }

    /// What pressing the row sends.
    pub fn on_press(mut self, m: M) -> Self {
        self.on_press = Some(m);
        self
    }

    pub fn height(mut self, h: impl Into<Length>) -> Self {
        self.height = h.into();
        self
    }
}

impl<'a, K: Clone + PartialEq + 'static, M: Clone + 'a> From<MediaListRow<'a, K, M>>
    for Element<'a, M>
{
    fn from(r: MediaListRow<'a, K, M>) -> Self {
        let layout = r.layout;
        let compact = layout.compact;
        let mut slots = layout.slots.iter();
        let title_slot = slots.next();

        let mut text = column![ty::body(r.title)];
        if !compact && let Some(s) = r.subtitle.clone() {
            text = text.push(ty::caption(s));
        }
        let mut head = row![].spacing(GAP).align_y(Alignment::Center);
        if layout.picture
            && let Some(p) = r.picture
        {
            head = head.push(container(p).width(PICTURE));
        }
        head = head.push(text);
        let mut line = row![
            container(head)
                .width(title_slot.map_or(Length::Fill, |s| s.width))
                .center_y(Length::Fill)
        ]
        .spacing(GAP)
        .align_y(Alignment::Center);

        // The subtitle's own column when compact, then the rest in order.
        let mut subtitle = r.subtitle;
        let mut cells = r.cells.into_iter();
        for s in slots {
            let content: Element<'a, M> = if compact && s.key.is_some() && subtitle.is_some() {
                ty::caption(subtitle.take().unwrap_or_default()).into()
            } else {
                cells.next().unwrap_or_else(|| space::horizontal().into())
            };
            line = line.push(
                container(content)
                    .width(s.width)
                    .align_x(s.align)
                    .center_y(Length::Fill),
            );
        }
        // The menu's slot is inside the button, so the highlight covers the whole row; the menu
        // itself is laid over it.
        if layout.menu {
            line = line.push(space::horizontal().width(MENU_W));
        }

        let body = container(line).height(Length::Fill).center_y(Length::Fill);
        let mut pressable = button(body)
            .padding([0.0, INSET]) // gutter-ok: the row's inset inside its highlight
            .width(Length::Fill)
            .height(r.height)
            .style(iad::style::button::flat);
        if let Some(m) = r.on_press {
            pressable = pressable.on_press(m);
        }
        match (layout.menu, r.menu) {
            (true, Some(menu)) => stack![
                pressable,
                container(menu)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Horizontal::Right)
                    .center_y(Length::Fill)
                    .padding([0.0, INSET]) // gutter-ok: the row's inset
            ]
            .height(r.height)
            .into(),
            _ => pressable.into(),
        }
    }
}
