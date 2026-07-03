use crate::app::{LauncherApp, Message, Tab};
use crate::constants::{AUTH_SCHEMES, PROVIDERS};
use crate::ui::styles::{
    danger_btn_style, ghost_btn_style, outline_btn_style, primary_btn_style, secondary_btn_style,
    CLR_BORDER, CLR_CARD, CLR_DANGER, CLR_SIDEBAR, CLR_SUCCESS, CLR_TEXT, CLR_TEXT_DIM,
    CLR_WARNING,
};
use iced::font::Weight;
use iced::widget::{
    button, checkbox, column, container, pick_list, row, rule, scrollable, text, text_input,
};
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

    // ── 進階設定 (Per-feature 開關) ──
    let advanced_title = text("進階設定").size(18).font(Font {
        weight: Weight::Semibold,
        ..Default::default()
    });

    let transport_options = vec!["openai_chat".to_string(), "anthropic_messages".to_string()];
    let reasoning_options = vec!["separate".to_string(), "inline".to_string()];

    let mut advanced_form = column![
        form_row(
            "傳輸協定",
            pick_list(
                transport_options,
                Some(app.transport_type.clone()),
                Message::TransportTypeSelected,
            )
            .width(Length::Fill)
            .into(),
        ),
        form_row(
            "Thinking 模式",
            pick_list(
                reasoning_options,
                Some(app.reasoning_replay_mode.clone()),
                Message::ReasoningReplayModeSelected,
            )
            .width(Length::Fill)
            .into(),
        ),
        // Per-feature toggles
        checkbox(app.enable_quota_check_mock)
            .label("配額檢查攔截")
            .on_toggle(Message::QuotaCheckMockToggled)
            .text_size(14)
            .spacing(8),
        checkbox(app.enable_prefix_detection)
            .label("命令前綴快速檢測")
            .on_toggle(Message::PrefixDetectionToggled)
            .text_size(14)
            .spacing(8),
        checkbox(app.enable_title_generation_skip)
            .label("標題生成跳過")
            .on_toggle(Message::TitleGenerationSkipToggled)
            .text_size(14)
            .spacing(8),
        checkbox(app.enable_suggestion_mode_skip)
            .label("建議模式跳過")
            .on_toggle(Message::SuggestionModeSkipToggled)
            .text_size(14)
            .spacing(8),
        checkbox(app.enable_filepath_extraction_mock)
            .label("檔案路徑提取模擬")
            .on_toggle(Message::FilepathExtractionMockToggled)
            .text_size(14)
            .spacing(8),
        checkbox(app.enable_safety_classifier_handling)
            .label("安全分類器處理")
            .on_toggle(Message::SafetyClassifierHandlingToggled)
            .text_size(14)
            .spacing(8),
        checkbox(app.enable_web_server_tools)
            .label("Web 工具攔截 (本地執行 web_search / web_fetch)")
            .on_toggle(Message::WebServerToolsToggled)
            .text_size(14)
            .spacing(8),
    ]
    .spacing(10);

    if app.enable_web_server_tools {
        advanced_form = advanced_form.push(
            row![
                text("     ") // 縮排
                    .width(Length::Fixed(20.0)),
                column![
                    checkbox(app.web_fetch_allow_private_networks)
                        .label("允許 web_fetch 存取私有網路目標")
                        .on_toggle(Message::WebFetchPrivateNetworkToggled)
                        .text_size(14)
                        .spacing(8),
                    form_row(
                        "允許的 URL 方案",
                        text_input("http,https", &app.web_fetch_allowed_schemes)
                            .on_input(Message::WebFetchAllowedSchemesChanged)
                            .padding(10)
                            .size(14)
                            .into(),
                    )
                ]
                .spacing(10)
                .width(Length::Fill)
            ]
            .spacing(0),
        );
    }

    // ── 組裝主要內容（依目前分頁切換） ──
    let tab_content: Element<'_, Message> = match app.current_tab {
        Tab::General => column![section_title, rule::horizontal(1), form, custom_section,]
            .spacing(18)
            .into(),
        Tab::Advanced => column![advanced_title, rule::horizontal(1), advanced_form,]
            .spacing(18)
            .into(),
    };

    let mut content = column![header, status_card, tab_content,]
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
        .padding([10, 20]),
        button(text("💾 僅儲存").size(14))
            .on_press(Message::SaveOnly)
            .style(secondary_btn_style)
            .padding([10, 16]),
        button(text("↩ 還原官方").size(14).color(CLR_TEXT_DIM))
            .on_press(Message::RestoreRequested)
            .style(outline_btn_style)
            .padding([10, 16]),
    ]
    .spacing(10);

    content = content.push(buttons);

    // ── 側邊欄選單 ──
    let menu_items = vec![("連線設定", Tab::General), ("進階設定", Tab::Advanced)];

    let sidebar_items: Vec<Element<'_, Message>> = menu_items
        .into_iter()
        .map(|(label, tab)| {
            let is_active = app.current_tab == tab;
            let btn = button(
                text(label)
                    .size(14)
                    .color(if is_active { CLR_TEXT } else { CLR_TEXT_DIM })
                    .font(Font {
                        weight: if is_active {
                            Weight::Semibold
                        } else {
                            Weight::Medium
                        },
                        ..Default::default()
                    }),
            )
            .on_press(Message::TabSelected(tab))
            .style(
                move |theme: &iced::Theme, status: button::Status| -> button::Style {
                    if is_active {
                        primary_btn_style(theme, status)
                    } else {
                        ghost_btn_style(theme, status)
                    }
                },
            )
            .padding([8, 12])
            .width(Length::Fill);
            btn.into()
        })
        .collect();

    let sidebar = container(
        column![
            text("設定選單").size(14).color(CLR_TEXT_DIM).font(Font {
                weight: Weight::Semibold,
                ..Default::default()
            }),
            column(sidebar_items).spacing(4),
        ]
        .spacing(12),
    )
    .padding([20, 12])
    .width(Length::Fixed(160.0))
    .height(Length::Fill)
    .style(|_theme| container::Style {
        text_color: None,
        background: Some(Background::Color(CLR_SIDEBAR)),
        border: Border {
            radius: 0.0.into(),
            width: 0.0,
            color: iced::Color::TRANSPARENT,
        },
        shadow: Shadow::default(),
        snap: false,
    });

    // ── 外層容器 ──
    let main_content = row![
        sidebar,
        scrollable(container(content).padding(30).center_x(Length::Fill)).width(Length::Fill),
    ]
    .spacing(0)
    .height(Length::Fill);

    main_content.into()
}
