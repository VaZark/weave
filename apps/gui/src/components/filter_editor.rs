//! Reusable filter editor layout. Context supplies content and commit behavior.
use iced::widget::{column, row, scrollable, space};
use iced::{Alignment, Length};
use iced_adaptive_design::{
    Element, Icon,
    prelude::{self as w, typography as ty},
};

pub struct FilterEditor<'a, M> {
    title: String,
    subtitle: Option<String>,
    body: Element<'a, M>,
    close: M,
    primary: M,
    label: String,
    actions: Vec<Element<'a, M>>,
    cancel: Option<M>,
}
impl<'a, M: Clone + 'a> FilterEditor<'a, M> {
    pub fn new(
        title: impl Into<String>,
        body: Element<'a, M>,
        close: M,
        label: impl Into<String>,
        primary: M,
    ) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            body,
            close,
            primary,
            label: label.into(),
            actions: Vec::new(),
            cancel: None,
        }
    }
    pub fn subtitle(mut self, text: impl Into<String>) -> Self {
        self.subtitle = Some(text.into());
        self
    }
    pub fn action(mut self, action: Element<'a, M>) -> Self {
        self.actions.push(action);
        self
    }
    pub fn cancel(mut self, cancel: M) -> Self {
        self.cancel = Some(cancel);
        self
    }
}
impl<'a, M: Clone + 'a> From<FilterEditor<'a, M>> for Element<'a, M> {
    fn from(editor: FilterEditor<'a, M>) -> Self {
        let mut heading = column![ty::title_3(editor.title)]
            .spacing(4)
            .width(Length::Fill);
        if let Some(subtitle) = editor.subtitle {
            heading = heading.push(ty::caption(subtitle));
        }
        let header = row![
            heading,
            w::tooltip(
                w::icon_button(Icon::WindowClose).on_press(editor.close),
                "Close filters",
                w::TooltipPosition::Bottom
            )
        ]
        .spacing(8)
        .align_y(Alignment::Start);
        let mut footer = row(editor.actions)
            .spacing(8)
            .align_y(Alignment::Center)
            .push(space::horizontal());
        if let Some(cancel) = editor.cancel {
            footer = footer.push(w::tooltip(
                w::icon_button(Icon::WindowClose).on_press(cancel),
                "Cancel",
                w::TooltipPosition::Top,
            ));
        }
        footer = footer.push(w::tooltip(
            w::suggested_button(Icon::Check.view(16.0)).on_press(editor.primary),
            editor.label,
            w::TooltipPosition::Top,
        ));
        column![
            header,
            scrollable(editor.body)
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::new().width(4).scroller_width(4)
                ))
                .height(Length::Fill),
            footer,
        ]
        .spacing(12)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
