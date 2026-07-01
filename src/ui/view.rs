use crate::app::{LauncherApp, Message};
use crate::constants::{AUTH_SCHEMES, PROVIDERS};
use crate::ui::styles::{
    danger_btn_style, ghost_btn_style, outline_btn_style, primary_btn_style, secondary_btn_style,
    CLR_BORDER, CLR_CARD, CLR_DANGER, CLR_SUCCESS, CLR_TEXT_DIM, CLR_WARNING,
};
use iced::font::Weight;
use iced::widget::{button, checkbox, column, container, pick_list, row, rule, text, text_input};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Shadow};

/// 表單列：左側標籤 + 右側控件
pub fn form_row<'a>(label: &str, widget: Element<'a, Message>) -> Element<'a, Message> {
    row![
        text(label.to_string())
            .size(14)
            .font(Font {
                weight: Weight::Semibold,
                ..Default::default()
            })
            .width(110),
        widget,
    ]
    .spacing(12)
    .align_y(Alignment::Center)
    .into()
}

pub fn view(app: &LauncherApp) -> Element<'_, Message> {
    // ── 標題區 ──
    let header = column![
        text("Free Claude Launcher").size(28).font(Font {
            weight: Weight::Bold,
            ..Default::default()
        }),
        text(format!("本機 Proxy：127.0.0.1:{}", app.current_port))
            .size(13)
            .color(CLR_TEXT_DIM),
    ]
    .spacing(4);

    // ── 狀態卡片 ──
    let status_color = if app.status_ok {
        CLR_SUCCESS
    } else {
        CLR_WARNING
    };
    let status_lines: Vec<&str> = app.status_text.lines().collect();

    let mut status_col = column![
        text(status_lines.first().copied().unwrap_or("").to_string())
            .size(14)
            .color(status_color)
            .font(Font {
                weight: Weight::Semibold,
                ..Default::default()
            })
    ]
    .spacing(3);

    if let Some(path_line) = status_lines.get(1) {
        status_col = status_col.push(text(path_line.to_string()).size(12).color(CLR_TEXT_DIM));
    }

    let status_card = container(status_col)
        .style(|_theme| container::Style {
            text_color: None,
            background: Some(Background::Color(CLR_CARD)),
            border: Border {
                radius: 10.0.into(),
                width: 1.0,
                color: CLR_BORDER,
            },
            shadow: Shadow::default(),
            snap: false,
        })
        .padding([14, 18])
        .width(Length::Fill);

    // ── 區段標題 ──
    let section_title = text("連線設定").size(18).font(Font {
        weight: Weight::Semibold,
        ..Default::default()
    });

    // ── 表單 ──
    let provider_options: Vec<String> = PROVIDERS.iter().map(|s| s.to_string()).collect();
    let auth_options: Vec<String> = AUTH_SCHEMES.iter().map(|s| s.to_string()).collect();

    let form = column![
        form_row(
            "API 供應商",
            pick_list(
                provider_options,
                app.provider.clone(),
                Message::ProviderSelected,
            )
            .placeholder("選擇供應商...")
            .width(Length::Fill)
            .into(),
        ),
        form_row(
            "Gateway URL",
            text_input("https://...", &app.base_url)
                .on_input(Message::BaseUrlChanged)
                .padding(10)
                .size(14)
                .into(),
        ),
        form_row(
            "API Key",
            text_input(&app.api_key_placeholder, &app.api_key)
                .on_input(Message::ApiKeyChanged)
                .secure(true)
                .padding(10)
                .size(14)
                .into(),
        ),
        form_row(
            "驗證方式",
            pick_list(
                auth_options,
                app.auth_scheme.clone(),
                Message::AuthSchemeSelected,
            )
            .width(Length::Fill)
            .into(),
        ),
    ]
    .spacing(14);

    // ── 自訂路徑 ──
    let mut custom_input = text_input("C:\\Users\\...\\Claude.exe", &app.custom_path)
        .padding(10)
        .size(14);
    if app.use_custom_path {
        custom_input = custom_input.on_input(Message::CustomPathChanged);
    }

    let custom_section = column![
        checkbox(app.use_custom_path)
            .label("使用自訂 Claude.exe 路徑")
            .on_toggle(Message::CustomPathToggled)
            .text_size(14)
            .spacing(8),
        custom_input,
    ]
    .spacing(8);

    // ── 組裝主要內容 ──
    let mut content = column![
        header,
        status_card,
        section_title,
        rule::horizontal(1),
        form,
        custom_section,
    ]
    .spacing(18)
    .max_width(540);

    // ── Toast 通知 ──
    if let Some(ref toast) = app.toast {
        let (bg, border_clr) = if toast.is_success {
            (Color::from_rgba(0.298, 0.831, 0.494, 0.10), CLR_SUCCESS)
        } else {
            (Color::from_rgba(1.0, 0.380, 0.380, 0.10), CLR_DANGER)
        };
        let txt_clr = if toast.is_success {
            CLR_SUCCESS
        } else {
            CLR_DANGER
        };

        let toast_widget = container(
            row![
                text(toast.message.clone())
                    .size(13)
                    .color(txt_clr)
                    .width(Length::Fill),
                button(text("✕").size(14).color(CLR_TEXT_DIM))
                    .on_press(Message::DismissToast)
                    .style(ghost_btn_style)
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

        content = content.push(toast_widget);
    }

    // ── 還原確認列 ──
    if app.confirming_restore {
        let confirm_bar = container(
            row![
                text("⚠ 確定要還原為官方設定？將移除 Gateway 設定。")
                    .size(13)
                    .color(CLR_WARNING)
                    .width(Length::Fill),
                button(text("確定").size(13).color(iced::Color::WHITE))
                    .on_press(Message::ConfirmRestore)
                    .style(danger_btn_style)
                    .padding([6, 16]),
                button(text("取消").size(13).color(CLR_TEXT_DIM))
                    .on_press(Message::CancelRestore)
                    .style(secondary_btn_style)
                    .padding([6, 16]),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .style(|_theme| container::Style {
            text_color: None,
            background: Some(Background::Color(iced::Color::from_rgba(
                1.0, 0.694, 0.298, 0.08,
            ))),
            border: Border {
                radius: 8.0.into(),
                width: 1.0,
                color: CLR_WARNING,
            },
            shadow: Shadow::default(),
            snap: false,
        })
        .padding([10, 14])
        .width(Length::Fill);

        content = content.push(confirm_bar);
    }

    // ── 底部按鈕列 ──
    let buttons = row![
        button(text("🚀 儲存並啟動 Claude").size(14).font(Font {
            weight: Weight::Semibold,
            ..Default::default()
        }))
        .on_press(Message::SaveAndLaunch)
        .style(primary_btn_style)
        .padding([10, 24]),
        button(text("💾 僅儲存").size(14))
            .on_press(Message::SaveOnly)
            .style(secondary_btn_style)
            .padding([10, 20]),
        button(text("↩ 還原官方").size(14).color(CLR_TEXT_DIM))
            .on_press(Message::RestoreRequested)
            .style(outline_btn_style)
            .padding([10, 20]),
    ]
    .spacing(10);

    content = content.push(buttons);

    // ── 外層容器 ──
    container(content).padding(30).center_x(Length::Fill).into()
}
