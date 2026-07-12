use super::components::{get_app_icon, shorten_path};
use crate::app::{LauncherApp, Message, Tab, ThemeMode};
use crate::ui::styles::{
    ColorPalette, custom_menu_style, custom_pick_list_style, custom_sidebar_btn_style,
    danger_btn_style, ghost_btn_style, outline_btn_style, primary_btn_style, secondary_btn_style,
    segmented_button_style,
};
use iced::font::Weight;
use iced::widget::{Space, button, column, container, image, pick_list, row, svg, text};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Shadow};

static SYSTEM_SVG: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><rect x=\"2\" y=\"3\" width=\"20\" height=\"14\" rx=\"2\" ry=\"2\"/><line x1=\"8\" y1=\"21\" x2=\"16\" y2=\"21\"/><line x1=\"12\" y1=\"17\" x2=\"12\" y2=\"21\"/></svg>";
static SUN_SVG: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><circle cx=\"12\" cy=\"12\" r=\"5\"/><line x1=\"12\" y1=\"1\" x2=\"12\" y2=\"3\"/><line x1=\"12\" y1=\"21\" x2=\"12\" y2=\"23\"/><line x1=\"4.22\" y1=\"4.22\" x2=\"5.64\" y2=\"5.64\"/><line x1=\"18.36\" y1=\"18.36\" x2=\"19.78\" y2=\"19.78\"/><line x1=\"1\" y1=\"12\" x2=\"3\" y2=\"12\"/><line x1=\"21\" y1=\"12\" x2=\"23\" y2=\"12\"/><line x1=\"4.22\" y1=\"19.78\" x2=\"5.64\" y2=\"18.36\"/><line x1=\"18.36\" y1=\"5.64\" x2=\"19.78\" y2=\"4.22\"/></svg>";
static MOON_SVG: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><path d=\"M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z\"/></svg>";

pub(super) fn header_and_status<'a>(
    app: &LauncherApp,
    palette: ColorPalette,
) -> (Element<'a, Message>, Element<'a, Message>) {
    let is_sys_active = app.theme_mode == ThemeMode::System;
    let is_light_active = app.theme_mode == ThemeMode::Light;
    let is_dark_active = app.theme_mode == ThemeMode::Dark;

    let sys_icon_color = if is_sys_active {
        Color::WHITE
    } else {
        palette.text_dim
    };
    let light_icon_color = if is_light_active {
        Color::WHITE
    } else {
        palette.text_dim
    };
    let dark_icon_color = if is_dark_active {
        Color::WHITE
    } else {
        palette.text_dim
    };

    // ── 頂部三態主題切換按鈕 (Claude 溫暖極簡風格 SVG 向量切換按鈕：系統 🖥 | 淺色 ☀ | 深色 ☽) ──
    let theme_buttons = container(
        row![
            button(
                svg(svg::Handle::from_memory(SYSTEM_SVG))
                    .width(14)
                    .height(14)
                    .style(move |_theme, _status| svg::Style {
                        color: Some(sys_icon_color)
                    })
            )
            .on_press(Message::ThemeModeSelected(ThemeMode::System))
            .style(move |_theme, status| { segmented_button_style(palette, is_sys_active, status) })
            .padding([6, 12]),
            button(
                svg(svg::Handle::from_memory(SUN_SVG))
                    .width(14)
                    .height(14)
                    .style(move |_theme, _status| svg::Style {
                        color: Some(light_icon_color)
                    })
            )
            .on_press(Message::ThemeModeSelected(ThemeMode::Light))
            .style(move |_theme, status| {
                segmented_button_style(palette, is_light_active, status)
            })
            .padding([6, 12]),
            button(
                svg(svg::Handle::from_memory(MOON_SVG))
                    .width(14)
                    .height(14)
                    .style(move |_theme, _status| svg::Style {
                        color: Some(dark_icon_color)
                    })
            )
            .on_press(Message::ThemeModeSelected(ThemeMode::Dark))
            .style(move |_theme, status| {
                segmented_button_style(palette, is_dark_active, status)
            })
            .padding([6, 12]),
        ]
        .spacing(2)
        .align_y(Alignment::Center),
    )
    .style(move |_theme| container::Style {
        text_color: None,
        background: Some(Background::Color(palette.segmented_bg)),
        border: Border {
            radius: 12.0.into(),
            width: 1.0,
            color: palette.border,
        },
        shadow: Shadow::default(),
        snap: false,
    })
    .padding(2);

    // ── 標題區 ──
    let header = row![
        column![
            text(app.language.tr("title"))
                .size(32)
                .color(palette.text)
                .font(Font {
                    weight: Weight::Bold,
                    ..Default::default()
                }),
            text(format!(
                "{}{}",
                app.language.tr("local_proxy"),
                app.current_port
            ))
            .size(14)
            .color(palette.text_dim),
        ]
        .spacing(4)
        .width(Length::Fill),
        theme_buttons,
    ]
    .align_y(Alignment::Center);

    // ── 狀態卡片 ──
    let status_color = if app.status_ok {
        palette.success
    } else {
        palette.warning
    };
    let status_lines: Vec<&str> = app.status_text.lines().collect();
    let status_first_line = status_lines.first().copied().unwrap_or("");
    let status_title_text = if status_first_line.contains("已偵測") {
        app.language.tr("detected_claude")
    } else if status_first_line.contains("尚未找到") {
        app.language.tr("not_found_claude")
    } else if status_first_line.contains("正在偵測") {
        app.language.tr("detecting")
    } else {
        status_first_line
    };

    let dot = text("●").size(14).color(status_color);

    let status_title = row![
        dot,
        text(status_title_text.to_string())
            .size(14)
            .color(palette.text)
            .font(Font {
                weight: Weight::Semibold,
                ..Default::default()
            })
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let mut status_col = column![status_title].spacing(4);

    if let Some(path_line) = status_lines.get(1) {
        status_col = status_col.push(
            text(shorten_path(path_line, 60))
                .size(13)
                .color(palette.text_dim),
        );
    }

    let status_card = container(status_col)
        .style(move |_theme| container::Style {
            text_color: None,
            background: Some(Background::Color(palette.card)),
            border: Border {
                radius: 8.0.into(),
                width: 1.0,
                color: palette.border,
            },
            shadow: Shadow::default(),
            snap: false,
        })
        .padding([12, 16])
        .width(Length::Fill);
    (header.into(), status_card.into())
}

pub(super) fn toast<'a>(app: &LauncherApp, palette: ColorPalette) -> Option<Element<'a, Message>> {
    if let Some(ref toast) = app.toast {
        let (bg, border_clr) = if toast.is_success {
            (Color::from_rgba(0.310, 0.522, 0.349, 0.10), palette.success)
        } else {
            (Color::from_rgba(0.788, 0.290, 0.161, 0.10), palette.danger)
        };
        let txt_clr = if toast.is_success {
            palette.success
        } else {
            palette.danger
        };

        let toast_widget = container(
            row![
                text(toast.message.clone())
                    .size(13)
                    .color(txt_clr)
                    .width(Length::Fill),
                button(text("✕").size(14).color(palette.text_dim))
                    .on_press(Message::DismissToast)
                    .style(move |_theme, status| ghost_btn_style(palette, status))
                    .padding([2, 8]),
            ]
            .align_y(Alignment::Center),
        )
        .style(move |_theme| container::Style {
            text_color: None,
            background: Some(Background::Color(bg)),
            border: Border {
                radius: 8.0.into(),
                width: 1.0,
                color: border_clr,
            },
            shadow: Shadow::default(),
            snap: false,
        })
        .padding([10, 14])
        .width(Length::Fill);
        Some(toast_widget.into())
    } else {
        None
    }
}

pub(super) fn confirm_bar<'a>(
    app: &LauncherApp,
    palette: ColorPalette,
) -> Option<Element<'a, Message>> {
    if app.confirming_restore {
        let confirm_bar = container(
            row![
                text(app.language.tr("reset_confirm_msg"))
                    .size(13)
                    .color(palette.warning)
                    .width(Length::Fill),
                button(
                    text(app.language.tr("confirm_reset"))
                        .size(13)
                        .color(iced::Color::WHITE)
                )
                .on_press(Message::ConfirmRestore)
                .style(move |_theme, status| danger_btn_style(palette, status))
                .padding([6, 16]),
                button(
                    text(app.language.tr("cancel"))
                        .size(13)
                        .color(palette.text_dim)
                )
                .on_press(Message::CancelRestore)
                .style(move |_theme, status| secondary_btn_style(palette, status))
                .padding([6, 16]),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .style(move |_theme| container::Style {
            text_color: None,
            background: Some(Background::Color(iced::Color::from_rgba(
                palette.warning.r,
                palette.warning.g,
                palette.warning.b,
                0.08,
            ))),
            border: Border {
                radius: 8.0.into(),
                width: 1.0,
                color: palette.warning,
            },
            shadow: Shadow::default(),
            snap: false,
        })
        .padding([10, 14])
        .width(Length::Fill);
        Some(confirm_bar.into())
    } else {
        None
    }
}

pub(super) fn action_buttons<'a>(app: &LauncherApp, palette: ColorPalette) -> Element<'a, Message> {
    let mut save_launch = button(text(app.language.tr("save_launch")).size(15).font(Font {
        weight: Weight::Semibold,
        ..Default::default()
    }))
    .style(move |_theme, status| primary_btn_style(palette, status))
    .padding([10, 16]);
    let mut save_only = button(text(app.language.tr("save_only")).size(15))
        .style(move |_theme, status| secondary_btn_style(palette, status))
        .padding([10, 12]);
    let mut resync = button(text(app.language.tr("sync_from_official")).size(15))
        .style(move |_theme, status| secondary_btn_style(palette, status))
        .padding([10, 12]);
    let mut restore = button(
        text(app.language.tr("reset_mirror_profile"))
            .size(15)
            .color(palette.text_dim),
    )
    .style(move |_theme, status| outline_btn_style(palette, status))
    .padding([10, 12]);
    if !app.is_busy() {
        save_launch = save_launch.on_press(Message::SaveAndLaunch);
        save_only = save_only.on_press(Message::SaveOnly);
        resync = resync.on_press(Message::ResyncFromOfficial);
        restore = restore.on_press(Message::RestoreRequested);
    }
    let buttons = row![save_launch, save_only, resync, restore].spacing(10);
    buttons.into()
}

pub(super) fn sidebar<'a>(app: &LauncherApp, palette: ColorPalette) -> Element<'a, Message> {
    const LANGUAGES: &[&str] = &["English", "繁體中文"];
    let selected_lang = match app.language {
        crate::core::config::Language::En => "English",
        crate::core::config::Language::ZhTw => "繁體中文",
    };

    let lang_pick_list = pick_list(LANGUAGES, Some(selected_lang), |selected| {
        let lang = match selected {
            "繁體中文" => crate::core::config::Language::ZhTw,
            _ => crate::core::config::Language::En,
        };
        Message::LanguageSelected(lang)
    })
    .width(Length::Fill)
    .style(move |_theme, status| custom_pick_list_style(palette, status))
    .menu_style(move |_theme| custom_menu_style(palette));

    let menu_items = vec![
        (app.language.tr("connection_settings"), Tab::General),
        (app.language.tr("models_thinking"), Tab::Models),
        (app.language.tr("extensions_skills"), Tab::Extensions),
        (app.language.tr("optimizations"), Tab::Optimizations),
    ];

    let sidebar_items: Vec<Element<'_, Message>> = menu_items
        .into_iter()
        .map(|(label, tab)| {
            let is_active = app.current_tab == tab;

            let indicator = container("")
                .width(Length::Fixed(3.0))
                .height(Length::Fixed(18.0))
                .style(move |_theme| container::Style {
                    background: Some(Background::Color(if is_active {
                        palette.primary
                    } else {
                        Color::TRANSPARENT
                    })),
                    border: Border {
                        radius: 1.5.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                });

            let btn = button(text(label).size(15).font(Font {
                weight: if is_active {
                    Weight::Semibold
                } else {
                    Weight::Medium
                },
                ..Default::default()
            }))
            .on_press(Message::TabSelected(tab))
            .style(move |_theme, status| custom_sidebar_btn_style(palette, is_active, status))
            .padding([10, 14])
            .width(Length::Fill);

            let item = row![indicator, btn].spacing(6).align_y(Alignment::Center);

            item.into()
        })
        .collect();

    let icon_widget = image(get_app_icon().clone())
        .width(Length::Fixed(56.0))
        .height(Length::Fixed(56.0));

    let sidebar = container(
        column![
            row![
                icon_widget,
                column![
                    text("FreeClaudeDesktop")
                        .size(15)
                        .color(palette.text)
                        .font(Font {
                            weight: Weight::Bold,
                            ..Default::default()
                        }),
                    text(app.language.tr("settings_menu"))
                        .size(13)
                        .color(palette.text_dim),
                ]
                .spacing(2),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            column(sidebar_items).spacing(4),
            Space::new().height(Length::Fill),
            lang_pick_list,
        ]
        .spacing(20),
    )
    .padding([20, 12])
    .width(Length::Fixed(190.0))
    .height(Length::Fill)
    .style(move |_theme| container::Style {
        text_color: None,
        background: Some(Background::Color(palette.sidebar)),
        border: Border {
            radius: 0.0.into(),
            width: 0.0,
            color: iced::Color::TRANSPARENT,
        },
        shadow: Shadow::default(),
        snap: false,
    });
    sidebar.into()
}
