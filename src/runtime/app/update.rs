use super::{LauncherApp, Message, Toast};
use iced::{
    window::{self, Mode},
    Task,
};
use std::path::Path;

impl LauncherApp {
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ProviderSelected(provider) => {
                self.provider = Some(provider.clone());
                self.toast = None;
                match provider.as_str() {
                    "OpenRouter" => {
                        self.base_url = "https://openrouter.ai/api".into();
                        self.auth_scheme = Some("bearer".into());
                        self.transport_type = "openai_chat".to_string();
                    }
                    "NVIDIA" => {
                        self.base_url = "https://integrate.api.nvidia.com/v1".into();
                        self.auth_scheme = Some("bearer".into());
                        self.transport_type = "openai_chat".to_string();
                    }
                    _ => {}
                }
            }
            Message::BaseUrlChanged(url) => {
                self.base_url = url;
                self.toast = None;
            }
            Message::ApiKeyChanged(key) => {
                self.api_key = key;
                self.toast = None;
            }
            Message::AuthSchemeSelected(scheme) => {
                self.auth_scheme = Some(scheme);
                self.toast = None;
            }
            Message::CustomPathToggled(checked) => {
                self.use_custom_path = checked;
                self.toast = None;
            }
            Message::CustomPathChanged(path) => {
                self.custom_path = path;
                self.toast = None;
            }
            Message::SaveAndLaunch => {
                self.confirming_restore = false;
                match self.save_config() {
                    Ok(()) => {
                        self.reload_model_settings();
                        let custom = if self.use_custom_path {
                            Some(Path::new(&self.custom_path))
                        } else {
                            None
                        };
                        match crate::launch_claude(custom) {
                            Ok(_path) => {
                                self.api_key.clear();
                                self.api_key_placeholder = "已儲存 API Key，留空沿用".into();
                                self.refresh_status();
                                self.toast = None;
                                if let Some(id) = self.window_id {
                                    self.is_hidden = true;
                                    return window::set_mode(id, Mode::Hidden);
                                }
                            }
                            Err(e) => {
                                self.toast = Some(Toast {
                                    message: format!("❌ 啟動失敗：{e}"),
                                    is_success: false,
                                });
                            }
                        }
                    }
                    Err(e) => {
                        self.toast = Some(Toast {
                            message: format!("❌ 儲存失敗：{e}"),
                            is_success: false,
                        });
                    }
                }
            }
            Message::SaveOnly => {
                self.confirming_restore = false;
                match self.save_config() {
                    Ok(()) => {
                        self.reload_model_settings();
                        self.api_key.clear();
                        self.api_key_placeholder = "已儲存 API Key，留空沿用".into();
                        self.toast = Some(Toast {
                            message: "✅ 設定已寫入 Claude。".into(),
                            is_success: true,
                        });
                    }
                    Err(e) => {
                        self.toast = Some(Toast {
                            message: format!("❌ 儲存失敗：{e}"),
                            is_success: false,
                        });
                    }
                }
            }
            Message::ResyncFromOfficial => match crate::launcher::resync_from_official() {
                Ok(()) => {
                    self.toast = Some(Toast {
                        message: "✅ 已從原版 Claude 重新同步設定至鏡像目錄。".into(),
                        is_success: true,
                    });
                }
                Err(e) => {
                    self.toast = Some(Toast {
                        message: format!("❌ 同步失敗：{e}"),
                        is_success: false,
                    });
                }
            },
            Message::RestoreRequested => {
                self.confirming_restore = true;
                self.toast = None;
            }
            Message::ConfirmRestore => {
                self.confirming_restore = false;
                match crate::launcher::reset_mirror_profile() {
                    Ok(()) => {
                        self.api_key.clear();
                        self.api_key_placeholder = "輸入 API Key".into();
                        self.toast = Some(Toast {
                            message: "✅ 鏡像 Profile 目錄已重置。原版目錄完全不受影響。".into(),
                            is_success: true,
                        });
                    }
                    Err(e) => {
                        self.toast = Some(Toast {
                            message: format!("❌ 重置失敗：{e}"),
                            is_success: false,
                        });
                    }
                }
            }
            Message::CancelRestore => {
                self.confirming_restore = false;
            }
            Message::DismissToast => {
                self.toast = None;
            }
            Message::WindowOpened(id) => {
                self.window_id = Some(id);
            }
            Message::CloseRequested(id) => {
                self.window_id = Some(id);
                self.is_hidden = true;
                return window::set_mode(id, Mode::Hidden);
            }
            Message::TrayQuit => {
                if let Some(id) = self.window_id {
                    return window::close(id);
                }
            }
            Message::TrayShow => {
                self.is_hidden = false;
                if let Some(id) = self.window_id {
                    return Task::batch([
                        window::set_mode(id, Mode::Windowed),
                        window::gain_focus(id),
                    ]);
                }
            }
            Message::TrayHide => {
                self.is_hidden = true;
                if let Some(id) = self.window_id {
                    return window::set_mode(id, Mode::Hidden);
                }
            }
            Message::TabSelected(tab) => self.current_tab = tab,
            Message::ThemeModeSelected(mode) => {
                self.theme_mode = mode;
                self.save_theme_mode();
            }
            Message::RefreshModels => match self.save_config() {
                Ok(()) => {
                    self.reload_model_settings();
                    self.toast = Some(Toast {
                        message: format!("已更新模型列表：{} 個模型", self.discovered_models.len()),
                        is_success: true,
                    });
                }
                Err(e) => {
                    self.toast = Some(Toast {
                        message: format!("更新模型列表失敗：{e}"),
                        is_success: false,
                    });
                }
            },
            // Per-feature optimization toggles
            Message::QuotaCheckMockToggled(v) => self.enable_quota_check_mock = v,
            Message::PrefixDetectionToggled(v) => self.enable_prefix_detection = v,
            Message::TitleGenerationSkipToggled(v) => self.enable_title_generation_skip = v,
            Message::SuggestionModeSkipToggled(v) => self.enable_suggestion_mode_skip = v,
            Message::FilepathExtractionMockToggled(v) => self.enable_filepath_extraction_mock = v,
            Message::WebServerToolsToggled(v) => self.enable_web_server_tools = v,
            Message::WebFetchPrivateNetworkToggled(v) => self.web_fetch_allow_private_networks = v,
            Message::WebFetchAllowedSchemesChanged(v) => self.web_fetch_allowed_schemes = v,
            Message::ReasoningReplayModeSelected(v) => self.reasoning_replay_mode = v,
            Message::TransportTypeSelected(v) => self.transport_type = v,
            Message::ModelReasoningLevelSelected(model, level) => {
                if level == "auto" {
                    self.model_reasoning_overrides.remove(&model);
                } else {
                    self.model_reasoning_overrides.insert(model, level);
                }
                self.toast = None;
            }
            Message::Model1mToggled(model, enabled) => {
                if enabled {
                    self.model_1m_overrides.insert(model, true);
                } else {
                    self.model_1m_overrides.remove(&model);
                }
                self.toast = None;
            }
            Message::RealModelSelected(opt) => {
                self.real_model = opt;
                self.toast = None;
            }
            Message::RealModelSonnetSelected(opt) => {
                self.real_model_sonnet = opt;
                self.toast = None;
            }
            Message::RealModelOpusSelected(opt) => {
                self.real_model_opus = opt;
                self.toast = None;
            }
            Message::RealModelHaikuSelected(opt) => {
                self.real_model_haiku = opt;
                self.toast = None;
            }
        }
        Task::none()
    }
}
