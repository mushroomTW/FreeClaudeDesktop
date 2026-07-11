use super::components::form_row;
use crate::app::{LauncherApp, Message, Tab};
use crate::constants::{AUTH_SCHEMES, PROVIDERS};
use crate::ui::styles::{
    custom_checkbox_style, custom_menu_style, custom_pick_list_style, custom_text_input_style,
    secondary_btn_style, ColorPalette,
};
use iced::font::Weight;
use iced::widget::{button, checkbox, column, pick_list, row, rule, text, text_input};
use iced::{Alignment, Element, Font, Length};

pub(super) fn tab_content<'a>(
    app: &'a LauncherApp,
    palette: ColorPalette,
    status_card: Element<'a, Message>,
) -> Element<'a, Message> {
    // ── 區段標題 ──
    let section_title = text("連線設定").size(21).color(palette.text).font(Font {
        weight: Weight::Semibold,
        ..Default::default()
    });

    // ── 表單 ──
    let form = column![
        form_row(
            "API 供應商",
            pick_list(PROVIDERS, app.provider.as_deref(), |s| {
                Message::ProviderSelected(s.to_string())
            },)
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
            pick_list(AUTH_SCHEMES, app.auth_scheme.as_deref(), |s| {
                Message::AuthSchemeSelected(s.to_string())
            },)
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
                let is_1m_enabled = app.model_1m_overrides.get(model).copied().unwrap_or(false);
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
                        .on_toggle(move |enabled| Message::Model1mToggled(
                            model_id_1m.clone(),
                            enabled
                        ))
                        .style(move |_theme, status| custom_checkbox_style(palette, status)),
                    pick_list(MODEL_REASONING_OPTIONS, Some(selected), move |level| {
                        Message::ModelReasoningLevelSelected(model_id.clone(), level.to_string())
                    },)
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
    let mut refresh_button = button(text("抓模型列表").size(13))
        .style(move |_theme, status| secondary_btn_style(palette, status))
        .padding([6, 12]);
    if !app.is_busy() {
        refresh_button = refresh_button.on_press(Message::RefreshModels);
    }
    let model_reasoning_section = column![
        refresh_button,
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
            pick_list(TRANSPORT_OPTIONS, Some(app.transport_type.as_str()), |s| {
                Message::TransportTypeSelected(s.to_string())
            },)
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

    let mut extensions_form = column![checkbox(app.enable_web_server_tools)
        .label("Web 工具攔截 (本地執行 web_search / web_fetch)")
        .on_toggle(Message::WebServerToolsToggled)
        .text_size(14)
        .spacing(8)
        .style(move |_theme, status| custom_checkbox_style(palette, status)),]
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
        Tab::Models => column![models_title, models_form,].spacing(18).into(),
        Tab::Extensions => column![extensions_title, extensions_form,]
            .spacing(18)
            .into(),
        Tab::Optimizations => column![optimizations_title, optimizations_form,]
            .spacing(18)
            .into(),
    };
    tab_content
}
