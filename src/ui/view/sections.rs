use super::components::form_row;
use crate::app::{LauncherApp, Message, Tab};
use crate::constants::AUTH_SCHEMES;
use crate::ui::styles::{
    ColorPalette, custom_checkbox_style, custom_menu_style, custom_pick_list_style,
    custom_text_input_style, secondary_btn_style,
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
    let section_title = text(app.language.tr("connection_settings"))
        .size(21)
        .color(palette.text)
        .font(Font {
            weight: Weight::Semibold,
            ..Default::default()
        });

    // ── 表單 ──
    let form = column![
        form_row(
            app.language.tr("api_provider"),
            pick_list(
                app.provider_options.as_slice(),
                app.provider.as_ref(),
                |s| { Message::ProviderSelected(s.to_string()) },
            )
            .placeholder(app.language.tr("select_provider"))
            .width(Length::Fill)
            .style(move |_theme, status| custom_pick_list_style(palette, status))
            .menu_style(move |_theme| custom_menu_style(palette))
            .into(),
            palette.text,
        ),
        form_row(
            app.language.tr("api_url"),
            text_input("https://...", &app.base_url)
                .on_input(Message::BaseUrlChanged)
                .padding(10)
                .size(14)
                .style(move |_theme, status| custom_text_input_style(palette, status))
                .into(),
            palette.text,
        ),
        form_row(
            app.language.tr("api_key"),
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
            app.language.tr("auth_scheme"),
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
            .label(app.language.tr("use_custom_path"))
            .on_toggle(Message::CustomPathToggled)
            .text_size(14)
            .spacing(8)
            .style(move |_theme, status| custom_checkbox_style(palette, status)),
        custom_input,
    ]
    .spacing(8);

    // ── 分頁 1: 模型與思考 (Models) ──
    let models_title = text(app.language.tr("models_thinking"))
        .size(21)
        .color(palette.text)
        .font(Font {
            weight: Weight::Semibold,
            ..Default::default()
        });

    const TRANSPORT_OPTIONS: &[&str] = &["openai_chat", "anthropic_messages"];
    const MODEL_REASONING_OPTIONS: &[&str] = &["none", "low", "medium", "high", "max"];
    let model_reasoning_rows: Vec<Element<'_, Message>> = if app.discovered_models.is_empty() {
        vec![
            text(app.language.tr("no_models_fetched"))
                .size(13)
                .color(palette.text_dim)
                .into(),
        ]
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
                let is_visible = app
                    .model_visibility_overrides
                    .get(model)
                    .copied()
                    .unwrap_or(true);
                let model_id = model.clone();
                let model_id_1m = model.clone();
                let model_id_visibility = model.clone();
                row![
                    text(model.clone())
                        .size(13)
                        .color(palette.text)
                        .width(Length::Fill),
                    checkbox(is_visible)
                        .label(app.language.tr("show"))
                        .text_size(13)
                        .spacing(6)
                        .on_toggle(move |visible| Message::ModelVisibilityToggled(
                            model_id_visibility.clone(),
                            visible
                        ))
                        .style(move |_theme, status| custom_checkbox_style(palette, status)),
                    checkbox(is_1m_enabled)
                        .label(app.language.tr("context_1m"))
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
    let mut refresh_button = button(text(app.language.tr("fetch_model_list")).size(13))
        .style(move |_theme, status| secondary_btn_style(palette, status))
        .padding([6, 12]);
    if !app.is_busy() {
        refresh_button = refresh_button.on_press(Message::RefreshModels);
    }
    let model_reasoning_section = column![
        refresh_button,
        text(app.language.tr("model_reasoning_limit"))
            .size(14)
            .color(palette.text)
            .font(Font {
                weight: Weight::Semibold,
                ..Default::default()
            }),
        text(app.language.tr("model_override_tip"))
            .size(12)
            .color(palette.text_dim),
        column(model_reasoning_rows).spacing(8),
    ]
    .spacing(6);

    const REASONING_OPTIONS: &[&str] = &["separate", "inline"];
    let models_form = column![
        form_row(
            app.language.tr("opus_model"),
            pick_list(
                app.model_options.as_slice(),
                Some(
                    app.real_model_opus
                        .clone()
                        .unwrap_or_else(|| app.model_options[0].clone()),
                ),
                |selected| {
                    if selected == app.model_options[0] {
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
            app.language.tr("sonnet_model"),
            pick_list(
                app.model_options.as_slice(),
                Some(
                    app.real_model_sonnet
                        .clone()
                        .unwrap_or_else(|| app.model_options[0].clone()),
                ),
                |selected| {
                    if selected == app.model_options[0] {
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
            app.language.tr("haiku_model"),
            pick_list(
                app.model_options.as_slice(),
                Some(
                    app.real_model_haiku
                        .clone()
                        .unwrap_or_else(|| app.model_options[0].clone()),
                ),
                |selected| {
                    if selected == app.model_options[0] {
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
            app.language.tr("fallback_model"),
            pick_list(
                app.model_options.as_slice(),
                Some(
                    app.real_model
                        .clone()
                        .unwrap_or_else(|| app.model_options[0].clone()),
                ),
                |selected| {
                    if selected == app.model_options[0] {
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
            app.language.tr("transport_protocol"),
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
            app.language.tr("thinking_mode"),
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
    let extensions_title = text(app.language.tr("extensions_skills"))
        .size(21)
        .color(palette.text)
        .font(Font {
            weight: Weight::Semibold,
            ..Default::default()
        });

    let mut extensions_form = column![
        checkbox(app.enable_web_server_tools)
            .label(app.language.tr("web_tool_intercept"))
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
                        .label(app.language.tr("allow_private_network"))
                        .on_toggle(Message::WebFetchPrivateNetworkToggled)
                        .text_size(14)
                        .spacing(8)
                        .style(move |_theme, status| custom_checkbox_style(palette, status)),
                    form_row(
                        app.language.tr("allowed_url_schemes"),
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
    let optimizations_title = text(app.language.tr("optimizations"))
        .size(21)
        .color(palette.text)
        .font(Font {
            weight: Weight::Semibold,
            ..Default::default()
        });

    let optimizations_form = column![
        checkbox(app.enable_quota_check_mock)
            .label(app.language.tr("quota_check_mock"))
            .on_toggle(Message::QuotaCheckMockToggled)
            .text_size(14)
            .spacing(8)
            .style(move |_theme, status| custom_checkbox_style(palette, status)),
        checkbox(app.enable_prefix_detection)
            .label(app.language.tr("prefix_detection"))
            .on_toggle(Message::PrefixDetectionToggled)
            .text_size(14)
            .spacing(8)
            .style(move |_theme, status| custom_checkbox_style(palette, status)),
        checkbox(app.enable_title_generation_skip)
            .label(app.language.tr("title_generation_skip"))
            .on_toggle(Message::TitleGenerationSkipToggled)
            .text_size(14)
            .spacing(8)
            .style(move |_theme, status| custom_checkbox_style(palette, status)),
        checkbox(app.enable_suggestion_mode_skip)
            .label(app.language.tr("suggestion_mode_skip"))
            .on_toggle(Message::SuggestionModeSkipToggled)
            .text_size(14)
            .spacing(8)
            .style(move |_theme, status| custom_checkbox_style(palette, status)),
        checkbox(app.enable_filepath_extraction_mock)
            .label(app.language.tr("filepath_extraction_mock"))
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
