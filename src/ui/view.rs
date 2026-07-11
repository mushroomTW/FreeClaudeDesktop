mod chrome;
mod components;
mod sections;

use crate::app::{LauncherApp, Message};
use crate::ui::styles::{custom_scrollable_style, ColorPalette};
use iced::widget::{column, container, row, scrollable};
use iced::{Background, Border, Element, Length, Padding, Shadow};

#[cfg(test)]
mod shorten_path_tests;

pub fn view(app: &LauncherApp) -> Element<'_, Message> {
    let palette = ColorPalette::for_mode(app.theme_mode);
    let (header, status_card) = chrome::header_and_status(app, palette);
    let tab_content = sections::tab_content(app, palette, status_card);

    let mut content = column![header, tab_content].spacing(20).max_width(580);
    if let Some(toast_widget) = chrome::toast(app, palette) {
        content = content.push(toast_widget);
    }

    let buttons = chrome::action_buttons(app, palette);
    let sidebar = chrome::sidebar(app, palette);

    // 滾動內容區（包含 header 與主要分頁內容）
    let scrollable_content = scrollable(
        container(content)
            .padding(Padding {
                top: 24.0,
                right: 30.0,
                bottom: 24.0,
                left: 30.0,
            })
            .center_x(Length::Fill)
            .style(move |_theme| container::Style {
                text_color: None,
                background: Some(Background::Color(palette.bg)),
                border: Border::default(),
                shadow: Shadow::default(),
                snap: false,
            }),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(move |_theme, status| custom_scrollable_style(palette, status));

    let mut bottom_col = column![].spacing(10);
    if let Some(confirm_bar) = chrome::confirm_bar(app, palette) {
        bottom_col = bottom_col.push(confirm_bar);
    }
    bottom_col = bottom_col.push(buttons);

    // 1px 的頂部分隔線
    let divider = container("")
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(move |_theme| container::Style {
            background: Some(Background::Color(palette.border)),
            ..Default::default()
        });

    let bottom_bar = container(column![
        divider,
        container(bottom_col).padding(Padding {
            top: 16.0,
            right: 30.0,
            bottom: 16.0,
            left: 30.0,
        })
    ])
    .width(Length::Fill);

    // 右側大面板（滾動內容 + 固定底欄）
    let right_panel = column![scrollable_content, bottom_bar,]
        .height(Length::Fill)
        .width(Length::Fill);

    // ── 外層容器 ──
    let main_content = container(row![sidebar, right_panel,].spacing(0).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme| container::Style {
            text_color: None,
            background: Some(Background::Color(palette.bg)),
            border: Border::default(),
            shadow: Shadow::default(),
            snap: false,
        });

    main_content.into()
}
