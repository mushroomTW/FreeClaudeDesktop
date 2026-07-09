use crate::app::Message;
use iced::font::Weight;
use iced::widget::{row, text};
use iced::{Alignment, Element, Font};
use std::sync::OnceLock;

static APP_ICON: OnceLock<iced::widget::image::Handle> = OnceLock::new();

pub(super) fn get_app_icon() -> &'static iced::widget::image::Handle {
    APP_ICON.get_or_init(|| {
        let ico_data = include_bytes!("../../../icon.ico");
        if let Ok(img) = ::image::load_from_memory(ico_data) {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            iced::widget::image::Handle::from_rgba(w, h, rgba.into_raw())
        } else {
            iced::widget::image::Handle::from_rgba(0, 0, vec![])
        }
    })
}

/// 表單列：左側標籤 + 右側控件
pub(super) fn form_row<'a>(
    label: &str,
    widget: Element<'a, Message>,
    text_color: iced::Color,
) -> Element<'a, Message> {
    row![
        text(label.to_string())
            .size(15)
            .color(text_color)
            .font(Font {
                weight: Weight::Semibold,
                ..Default::default()
            })
            .width(140),
        widget,
    ]
    .spacing(12)
    .align_y(Alignment::Center)
    .into()
}

pub(super) fn shorten_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        return path.to_string();
    }
    // ponytail: 純 head/tail 截斷，用 char_indices 找 UTF-8 安全邊界。
    let half = max_len.saturating_sub(3) / 2;
    if half == 0 {
        return path.to_string();
    }
    let head_end = path
        .char_indices()
        .take_while(|(i, _)| *i < half)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    let tail_start = path.len().saturating_sub(half);
    let tail_start = path
        .char_indices()
        .skip_while(|(i, _)| *i < tail_start)
        .map(|(i, _)| i)
        .next()
        .unwrap_or(path.len());
    format!("{}...{}", &path[..head_end], &path[tail_start..])
}
