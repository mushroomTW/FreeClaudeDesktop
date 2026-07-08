use crate::app::{LauncherApp, Message, Tab, ThemeMode};
use crate::constants::{AUTH_SCHEMES, PROVIDERS};
use crate::ui::styles::{
    custom_checkbox_style, custom_menu_style, custom_pick_list_style, custom_scrollable_style,
    custom_text_input_style, danger_btn_style, ghost_btn_style, outline_btn_style,
    primary_btn_style, secondary_btn_style, segmented_button_style, ColorPalette,
};
use iced::font::Weight;
use iced::widget::{
    button, checkbox, column, container, image, pick_list, row, rule, scrollable, svg, text,
    text_input,
};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Padding, Shadow};
use std::sync::OnceLock;

static APP_ICON: OnceLock<iced::widget::image::Handle> = OnceLock::new();

static SYSTEM_SVG: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><rect x=\"2\" y=\"3\" width=\"20\" height=\"14\" rx=\"2\" ry=\"2\"/><line x1=\"8\" y1=\"21\" x2=\"16\" y2=\"21\"/><line x1=\"12\" y1=\"17\" x2=\"12\" y2=\"21\"/></svg>";
static SUN_SVG: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><circle cx=\"12\" cy=\"12\" r=\"5\"/><line x1=\"12\" y1=\"1\" x2=\"12\" y2=\"3\"/><line x1=\"12\" y1=\"21\" x2=\"12\" y2=\"23\"/><line x1=\"4.22\" y1=\"4.22\" x2=\"5.64\" y2=\"5.64\"/><line x1=\"18.36\" y1=\"18.36\" x2=\"19.78\" y2=\"19.78\"/><line x1=\"1\" y1=\"12\" x2=\"3\" y2=\"12\"/><line x1=\"21\" y1=\"12\" x2=\"23\" y2=\"12\"/><line x1=\"4.22\" y1=\"19.78\" x2=\"5.64\" y2=\"18.36\"/><line x1=\"18.36\" y1=\"5.64\" x2=\"19.78\" y2=\"4.22\"/></svg>";
static MOON_SVG: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><path d=\"M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z\"/></svg>";

pub fn get_app_icon() -> &'static iced::widget::image::Handle {
    APP_ICON.get_or_init(|| {
        let ico_data = include_bytes!("../../icon.ico");
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
pub fn form_row<'a>(
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

fn shorten_path(path: &str, max_len: usize) -> String {
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

#[cfg(test)]
mod shorten_path_tests {
    use super::shorten_path;

    #[test]
    fn short_path_unchanged() {
        assert_eq!(shorten_path("C:\\Users\\file.txt", 60), "C:\\Users\\file.txt");
    }

    #[test]
    fn long_path_truncates_safely() {
        let long = "C:\\Users\\mushroomMaster\\Documents\\FreeClaudeDesktop\\very\\deeply\\nested\\file.txt";
        let result = shorten_path(long, 30);
        assert!(result.len() <= 33); // head + "..." + tail
        assert!(result.contains("..."));
    }

    #[test]
    fn non_ascii_path_does_not_panic() {
        // 含中文的路徑，截斷點不能落在多位元組字元中間。
        let long = "C:\\Users\\中文字\\Desktop\\FreeClaudeDesktop\\super\\deeply\\nested\\file.txt";
        let result = shorten_path(long, 24);
        assert!(result.starts_with(|c: char| c.is_ascii()));
        assert!(result.contains("..."));
    }
}

pub fn view(app: &LauncherApp) -> Element<'_, Message> {
    let palette = ColorPalette::for_mode(app.theme_mode);

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
            text("Free Claude Launcher")
                .size(32)
                .color(palette.text)
                .font(Font {
                    weight: Weight::Bold,
                    ..Default::default()
                }),
            text(format!("本機 Proxy：127.0.0.1:{}", app.current_port))
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

    let dot = text("●")
        .size(14)
        .color(status_color);

    let status_title = row![
        dot,
        text(status_lines.first().copied().unwrap_or("").to_string())
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
                .color(palette.text_dim)
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

    // ── 區段標題 ──
    let section_title = text("連線設定").size(21).color(palette.text).font(Font {
        weight: Weight::Semibold,
        ..Default::default()
    });

    // ── 表單 ──
    let form = column![
        form_row(
            "API 供應商",
            pick_list(
                PROVIDERS,
                app.provider.as_deref(),
                |s| Message::ProviderSelected(s.to_string()),
            )
            .placeholder("選擇供應商...")
            .width(Length::Fill)
            .style(move |_theme, status| custom_pick_list_style(palette, status))
            .menu_style(move |_theme| custom_menu_style(palette))
            .into(),
            palette.text,
        ),
        form_row(
            "API URL",
            text_input("https://...", &app.base_url)
                .on_input(Message::BaseUrlChanged)
                .padding(10)
                .size(14)
                .style(move |_theme, status| custom_text_input_style(palette, status))
                .into(),
            palette.text,
        ),
        form_row(
            "API Key",
            text_input(&app.api_key_placeholder, &app.api_key)
                .on_input(Message::ApiKeyChanged)
                .secure(true)
                .padding(10)
                .size(14)
                .style(move |_theme, status| custom_text_input_style(palette, status))
                .into(),
            palette.text,
        ),
        form_row(
            "驗證方式",
            pick_list(
                AUTH_SCHEMES,
                app.auth_scheme.as_deref(),
                |s| Message::AuthSchemeSelected(s.to_string()),
            )
            .width(Length::Fill)
            .style(move |_theme, status| custom_pick_list_style(palette, status))
            .menu_style(move |_theme| custom_menu_style(palette))
            .into(),
            palette.text,
        ),
    ]
    .spacing(14);

    // ── 自訂路徑 ──
    let mut custom_input = text_input("C:\\Users\\...\\Claude.exe", &app.custom_path)
        .padding(10)
        .size(14)
        .style(move |_theme, status| custom_text_input_style(palette, status));
    if app.use_custom_path {
        custom_input = custom_input.on_input(Message::CustomPathChanged);
    }

    let custom_section = column![
        checkbox(app.use_custom_path)
            .label("使用自訂 Claude.exe 路徑")
            .on_toggle(Message::CustomPathToggled)
            .text_size(14)
            .spacing(8)
            .style(move |_theme, status| custom_checkbox_style(palette, status)),
        custom_input,
    ]
    .spacing(8);

    // ── 分頁 1: 模型與思考 (Models) ──
    let models_title = text("模型與思考").size(21).color(palette.text).font(Font {
        weight: Weight::Semibold,
        ..Default::default()
    });

    const TRANSPORT_OPTIONS: &[&str] = &["openai_chat", "anthropic_messages"];
    const MODEL_REASONING_OPTIONS: &[&str] = &["none", "low", "medium", "high", "max"];
    let model_reasoning_rows: Vec<Element<'_, Message>> = if app.discovered_models.is_empty() {
        vec![text("尚未抓到模型；儲存設定後會列出可設定的模型。")
            .size(13)
            .color(palette.text_dim)
            .into()]
    } else {
        app.discovered_models
            .iter()
            .map(|model| {
                let selected = app
                    .model_reasoning_overrides
                    .get(model)
                    .map(|s| s.as_str())
                    .unwrap_or("none");
                let is_1m_enabled = app
                    .model_1m_overrides
                    .get(model)
                    .copied()
                    .unwrap_or(false);
                let model_id = model.clone();
                let model_id_1m = model.clone();
                row![
                    text(model.clone())
                        .size(13)
                        .color(palette.text)
                        .width(Length::Fill),
                    checkbox(is_1m_enabled)
                        .label("1M 上下文")
                        .text_size(13)
                        .spacing(6)
                        .on_toggle(move |enabled| Message::Model1mToggled(model_id_1m.clone(), enabled))
                        .style(move |_theme, status| custom_checkbox_style(palette, status)),
                    pick_list(
                        MODEL_REASONING_OPTIONS,
                        Some(selected),
                        move |level| Message::ModelReasoningLevelSelected(model_id.clone(), level.to_string()),
                    )
                    .width(Length::Fixed(130.0))
                    .style(move |_theme, status| custom_pick_list_style(palette, status))
                    .menu_style(move |_theme| custom_menu_style(palette))
                ]
                .spacing(10)
                .align_y(Alignment::Center)
                .into()
            })
            .collect()
    };
    let model_reasoning_section = column![
        button(text("抓模型列表").size(13))
            .on_press(Message::RefreshModels)
            .style(move |_theme, status| secondary_btn_style(palette, status))
            .padding([6, 12]),
        text("模型思考上限")
            .size(14)
            .color(palette.text)
            .font(Font {
                weight: Weight::Semibold,
                ..Default::default()
            }),
        text("這裡的設定會覆寫本專案的 Claude Desktop 模型路由。")
            .size(12)
            .color(palette.text_dim),
        column(model_reasoning_rows).spacing(8),
    ]
    .spacing(6);

    const REASONING_OPTIONS: &[&str] = &["separate", "inline"];
    let models_form = column![
        form_row(
            "Opus 模型",
            pick_list(
                app.model_options.as_slice(),
                Some(
                    app.real_model_opus
                        .clone()
                        .unwrap_or_else(|| "(自動/動態別名)".to_string()),
                ),
                |selected| {
                    if selected == "(自動/動態別名)" {
                        Message::RealModelOpusSelected(None)
                    } else {
                        Message::RealModelOpusSelected(Some(selected))
                    }
                },
            )
            .width(Length::Fill)
            .style(move |_theme, status| custom_pick_list_style(palette, status))
            .menu_style(move |_theme| custom_menu_style(palette))
            .into(),
            palette.text,
        ),
        form_row(
            "Sonnet 模型",
            pick_list(
                app.model_options.as_slice(),
                Some(
                    app.real_model_sonnet
                        .clone()
                        .unwrap_or_else(|| "(自動/動態別名)".to_string()),
                ),
                |selected| {
                    if selected == "(自動/動態別名)" {
                        Message::RealModelSonnetSelected(None)
                    } else {
                        Message::RealModelSonnetSelected(Some(selected))
                    }
                },
            )
            .width(Length::Fill)
            .style(move |_theme, status| custom_pick_list_style(palette, status))
            .menu_style(move |_theme| custom_menu_style(palette))
            .into(),
            palette.text,
        ),
        form_row(
            "Haiku 模型",
            pick_list(
                app.model_options.as_slice(),
                Some(
                    app.real_model_haiku
                        .clone()
                        .unwrap_or_else(|| "(自動/動態別名)".to_string()),
                ),
                |selected| {
                    if selected == "(自動/動態別名)" {
                        Message::RealModelHaikuSelected(None)
                    } else {
                        Message::RealModelHaikuSelected(Some(selected))
                    }
                },
            )
            .width(Length::Fill)
            .style(move |_theme, status| custom_pick_list_style(palette, status))
            .menu_style(move |_theme| custom_menu_style(palette))
            .into(),
            palette.text,
        ),
        form_row(
            "預設保底模型",
            pick_list(
                app.model_options.as_slice(),
                Some(
                    app.real_model
                        .clone()
                        .unwrap_or_else(|| "(自動/動態別名)".to_string()),
                ),
                |selected| {
                    if selected == "(自動/動態別名)" {
                        Message::RealModelSelected(None)
                    } else {
                        Message::RealModelSelected(Some(selected))
                    }
                },
            )
            .width(Length::Fill)
            .style(move |_theme, status| custom_pick_list_style(palette, status))
            .menu_style(move |_theme| custom_menu_style(palette))
            .into(),
            palette.text,
        ),
        form_row(
            "傳輸協定",
            pick_list(
                TRANSPORT_OPTIONS,
                Some(app.transport_type.as_str()),
                |s| Message::TransportTypeSelected(s.to_string()),
            )
            .width(Length::Fill)
            .style(move |_theme, status| custom_pick_list_style(palette, status))
            .menu_style(move |_theme| custom_menu_style(palette))
            .into(),
            palette.text,
        ),
        form_row(
            "Thinking 模式",
            pick_list(
                REASONING_OPTIONS,
                Some(app.reasoning_replay_mode.as_str()),
                |s| Message::ReasoningReplayModeSelected(s.to_string()),
            )
            .width(Length::Fill)
            .style(move |_theme, status| custom_pick_list_style(palette, status))
            .menu_style(move |_theme| custom_menu_style(palette))
            .into(),
            palette.text,
        ),
        rule::horizontal(1),
        model_reasoning_section,
    ]
    .spacing(14);

    // ── 分頁 2: 擴充與技能 (Extensions) ──
    let extensions_title = text("擴充與技能").size(21).color(palette.text).font(Font {
        weight: Weight::Semibold,
        ..Default::default()
    });

    let mut extensions_form = column![
        checkbox(app.enable_web_server_tools)
            .label("Web 工具攔截 (本地執行 web_search / web_fetch)")
            .on_toggle(Message::WebServerToolsToggled)
            .text_size(14)
            .spacing(8)
            .style(move |_theme, status| custom_checkbox_style(palette, status)),
    ]
    .spacing(10);

    if app.enable_web_server_tools {
        extensions_form = extensions_form.push(
            row![
                text("     ") // 縮排
                    .width(Length::Fixed(20.0)),
                column![
                    checkbox(app.web_fetch_allow_private_networks)
                        .label("允許 web_fetch 存取私有網路目標")
                        .on_toggle(Message::WebFetchPrivateNetworkToggled)
                        .text_size(14)
                        .spacing(8)
                        .style(move |_theme, status| custom_checkbox_style(palette, status)),
                    form_row(
                        "允許的 URL 方案",
                        text_input("http,https", &app.web_fetch_allowed_schemes)
                            .on_input(Message::WebFetchAllowedSchemesChanged)
                            .padding(10)
                            .size(14)
                            .style(move |_theme, status| custom_text_input_style(palette, status))
                            .into(),
                        palette.text,
                    )
                ]
                .spacing(10)
                .width(Length::Fill)
            ]
            .spacing(0),
        );
    }

    extensions_form = extensions_form.push(
        checkbox(app.enable_computer_mcp_server)
            .label("Computer MCP server (本機螢幕、滑鼠、鍵盤)")
            .on_toggle(Message::ComputerMcpServerToggled)
            .text_size(14)
            .spacing(8)
            .style(move |_theme, status| custom_checkbox_style(palette, status)),
    );

    // ── 分頁 3: 效能優化 (Optimizations) ──
    let optimizations_title = text("效能優化").size(21).color(palette.text).font(Font {
        weight: Weight::Semibold,
        ..Default::default()
    });

    let optimizations_form = column![
        checkbox(app.enable_quota_check_mock)
            .label("配額檢查攔截")
            .on_toggle(Message::QuotaCheckMockToggled)
            .text_size(14)
            .spacing(8)
            .style(move |_theme, status| custom_checkbox_style(palette, status)),
        checkbox(app.enable_prefix_detection)
            .label("命令前綴快速檢測")
            .on_toggle(Message::PrefixDetectionToggled)
            .text_size(14)
            .spacing(8)
            .style(move |_theme, status| custom_checkbox_style(palette, status)),
        checkbox(app.enable_title_generation_skip)
            .label("標題生成跳過")
            .on_toggle(Message::TitleGenerationSkipToggled)
            .text_size(14)
            .spacing(8)
            .style(move |_theme, status| custom_checkbox_style(palette, status)),
        checkbox(app.enable_suggestion_mode_skip)
            .label("建議模式跳過")
            .on_toggle(Message::SuggestionModeSkipToggled)
            .text_size(14)
            .spacing(8)
            .style(move |_theme, status| custom_checkbox_style(palette, status)),
        checkbox(app.enable_filepath_extraction_mock)
            .label("檔案路徑提取模擬")
            .on_toggle(Message::FilepathExtractionMockToggled)
            .text_size(14)
            .spacing(8)
            .style(move |_theme, status| custom_checkbox_style(palette, status)),
    ]
    .spacing(10);

    // ── 組裝主要內容（依目前分頁切換） ──
    let tab_content: Element<'_, Message> = match app.current_tab {
        Tab::General => column![section_title, form, custom_section, status_card,]
            .spacing(18)
            .into(),
        Tab::Models => column![models_title, models_form,]
            .spacing(18)
            .into(),
        Tab::Extensions => column![extensions_title, extensions_form,]
            .spacing(18)
            .into(),
        Tab::Optimizations => column![optimizations_title, optimizations_form,]
            .spacing(18)
            .into(),
    };

    let mut content = column![header, tab_content,]
        .spacing(20)
        .max_width(580);

    // ── Toast 通知 ──
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

        content = content.push(toast_widget);
    }

    // ── 重置鏡像確認列 ──
    let mut confirm_bar_widget: Option<Element<'_, Message>> = None;
    if app.confirming_restore {
        let confirm_bar = container(
            row![
                text("⚠ 確定要重置鏡像 Profile 目錄？原版目錄完全不受影響。")
                    .size(13)
                    .color(palette.warning)
                    .width(Length::Fill),
                button(text("確定重置").size(13).color(iced::Color::WHITE))
                    .on_press(Message::ConfirmRestore)
                    .style(move |_theme, status| danger_btn_style(palette, status))
                    .padding([6, 16]),
                button(text("取消").size(13).color(palette.text_dim))
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

        confirm_bar_widget = Some(confirm_bar.into());
    }

    // ── 底部按鈕列（現代極簡微符號風格） ──
    let buttons = row![
        button(text("儲存並啟動 ↵").size(15).font(Font {
            weight: Weight::Semibold,
            ..Default::default()
        }))
        .on_press(Message::SaveAndLaunch)
        .style(move |_theme, status| primary_btn_style(palette, status))
        .padding([12, 24]),
        button(text("僅儲存").size(15))
            .on_press(Message::SaveOnly)
            .style(move |_theme, status| secondary_btn_style(palette, status))
            .padding([12, 20]),
        button(text("從原版同步").size(15))
            .on_press(Message::ResyncFromOfficial)
            .style(move |_theme, status| secondary_btn_style(palette, status))
            .padding([12, 20]),
        button(text("重置鏡像 Profile").size(15).color(palette.text_dim))
            .on_press(Message::RestoreRequested)
            .style(move |_theme, status| outline_btn_style(palette, status))
            .padding([12, 20]),
    ]
    .spacing(10);

    // ── 側邊欄選單 (4 個主分頁) ──
    let menu_items = vec![
        ("連線設定", Tab::General),
        ("模型與思考", Tab::Models),
        ("擴充與技能", Tab::Extensions),
        ("效能優化", Tab::Optimizations),
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
            
            let btn = button(
                text(label)
                    .size(15)
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
            .style(move |_theme, status| {
                crate::ui::styles::custom_sidebar_btn_style(palette, is_active, status)
            })
            .padding([10, 14])
            .width(Length::Fill);

            let item = row![indicator, btn]
                .spacing(6)
                .align_y(Alignment::Center);

            item.into()
        })
        .collect();

    let icon_widget = image(get_app_icon().clone())
        .width(Length::Fixed(32.0))
        .height(Length::Fixed(32.0));

    let sidebar = container(
        column![
            row![
                icon_widget,
                column![
                    text("Free Claude").size(15).color(palette.text).font(Font {
                        weight: Weight::Bold,
                        ..Default::default()
                    }),
                    text("設定選單").size(12).color(palette.text_dim),
                ]
                .spacing(2),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            column(sidebar_items).spacing(4),
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
            })
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(move |_theme, status| custom_scrollable_style(palette, status));

    // ── 底部固定按鈕欄 ──
    let mut bottom_col = column![].spacing(10);
    if let Some(confirm_bar) = confirm_bar_widget {
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

    let bottom_bar = container(
        column![
            divider,
            container(bottom_col).padding(Padding {
                top: 16.0,
                right: 30.0,
                bottom: 16.0,
                left: 30.0,
            })
        ]
    )
    .width(Length::Fill);

    // 右側大面板（滾動內容 + 固定底欄）
    let right_panel = column![
        scrollable_content,
        bottom_bar,
    ]
    .height(Length::Fill)
    .width(Length::Fill);

    // ── 外層容器 ──
    let main_content = container(
        row![
            sidebar,
            right_panel,
        ]
        .spacing(0)
        .height(Length::Fill),
    )
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
