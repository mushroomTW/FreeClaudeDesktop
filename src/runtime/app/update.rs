use super::{ConfigAction, JobKind, LauncherApp, Message, Toast};
use iced::{
    Task,
    window::{self, Mode},
};

impl LauncherApp {
    fn start_config_job(&mut self, action: ConfigAction) -> Task<Message> {
        let kind = if action == ConfigAction::RefreshModels {
            JobKind::Refreshing
        } else {
            JobKind::Saving
        };
        let (id, registration) = self.jobs.begin(kind);
        let input = self.config_input();
        Task::perform(
            async move {
                let operation = async move {
                    if action == ConfigAction::RefreshModels {
                        crate::config_service::refresh_models_async(input).await
                    } else {
                        crate::config_service::save_config_async(input).await
                    }
                    .map_err(|error| error.to_string())
                };
                futures::future::Abortable::new(operation, registration)
                    .await
                    .unwrap_or_else(|_| Err("工作已取消".to_string()))
            },
            move |result| Message::ConfigFinished(id, action, result),
        )
    }

    fn start_launch_job(&mut self) -> Task<Message> {
        let (id, registration) = self.jobs.begin(JobKind::Launching);
        let custom_path = self
            .use_custom_path
            .then(|| std::path::PathBuf::from(self.custom_path.clone()));
        Task::perform(
            async move {
                let operation = async move {
                    tokio::task::spawn_blocking(move || {
                        crate::launch_claude(custom_path.as_deref())
                            .map_err(|error| error.to_string())
                    })
                    .await
                    .map_err(|error| error.to_string())?
                };
                futures::future::Abortable::new(operation, registration)
                    .await
                    .unwrap_or_else(|_| Err("工作已取消".to_string()))
            },
            move |result| Message::LaunchFinished(id, result),
        )
    }

    fn finish_unit_job(
        &mut self,
        id: u64,
        result: Result<(), String>,
        success: &str,
        operation: &str,
    ) {
        match result {
            Ok(()) if self.jobs.accept(id) => {
                self.toast = Some(Toast {
                    message: success.to_string(),
                    is_success: true,
                });
            }
            Err(error) if self.jobs.fail(id, error.clone()) => {
                let msg = if self.language == crate::core::config::Language::ZhTw {
                    format!("{operation}失敗：{error}")
                } else {
                    format!("{operation} failed: {error}")
                };
                self.toast = Some(Toast {
                    message: msg,
                    is_success: false,
                });
            }
            _ => {}
        }
    }

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
                return self.start_config_job(ConfigAction::SaveAndLaunch);
            }
            Message::SaveOnly => {
                self.confirming_restore = false;
                return self.start_config_job(ConfigAction::SaveOnly);
            }
            Message::ResyncFromOfficial => {
                let (id, registration) = self.jobs.begin(JobKind::Resyncing);
                return Task::perform(
                    async move {
                        let operation = async {
                            tokio::task::spawn_blocking(crate::launcher::resync_from_official)
                                .await
                                .map_err(|error| error.to_string())?
                                .map_err(|error| error.to_string())
                        };
                        futures::future::Abortable::new(operation, registration)
                            .await
                            .unwrap_or_else(|_| Err("工作已取消".to_string()))
                    },
                    move |result| Message::ResyncFinished(id, result),
                );
            }
            Message::RestoreRequested => {
                self.confirming_restore = true;
                self.toast = None;
            }
            Message::ConfirmRestore => {
                self.confirming_restore = false;
                let (id, registration) = self.jobs.begin(JobKind::Restoring);
                return Task::perform(
                    async move {
                        let operation = async {
                            tokio::task::spawn_blocking(crate::launcher::reset_mirror_profile)
                                .await
                                .map_err(|error| error.to_string())?
                                .map_err(|error| error.to_string())
                        };
                        futures::future::Abortable::new(operation, registration)
                            .await
                            .unwrap_or_else(|_| Err("工作已取消".to_string()))
                    },
                    move |result| Message::RestoreFinished(id, result),
                );
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
                self.jobs.cancel();
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
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            let mut settings = crate::get_launcher_settings().unwrap_or_default();
                            settings.theme_mode = mode.as_str().to_string();
                            crate::save_launcher_settings(&settings)
                                .map_err(|error| error.to_string())
                        })
                        .await
                        .map_err(|error| error.to_string())?
                    },
                    Message::ThemeSaved,
                );
            }
            Message::LanguageSelected(lang) => {
                self.language = lang;
                self.update_model_options();
                self.update_provider_options();
                if let Some(ref p) = self.provider {
                    if p == "自訂" || p == "Custom" {
                        self.provider = Some(self.language.tr("custom").to_string());
                    }
                }
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            let mut settings = crate::get_launcher_settings().unwrap_or_default();
                            settings.language = lang.as_str().to_string();
                            crate::save_launcher_settings(&settings)
                                .map_err(|error| error.to_string())
                        })
                        .await
                        .map_err(|error| error.to_string())?
                    },
                    Message::LanguageSaved,
                );
            }
            Message::LanguageSaved(Err(error)) => {
                let msg = if self.language == crate::core::config::Language::ZhTw {
                    format!("儲存語言設定失敗：{error}")
                } else {
                    format!("Failed to save language settings: {error}")
                };
                self.toast = Some(Toast {
                    message: msg,
                    is_success: false,
                });
            }
            Message::LanguageSaved(Ok(())) => {}
            Message::RefreshModels => return self.start_config_job(ConfigAction::RefreshModels),
            Message::ConfigFinished(id, action, result) => match result {
                Ok(output) if self.jobs.accept(id) => {
                    self.discovered_models = output.discovered_models;
                    self.update_model_options();
                    self.api_key.clear();
                    self.api_key_placeholder = self.language.tr("key_saved_tip").into();
                    if action == ConfigAction::SaveAndLaunch {
                        return self.start_launch_job();
                    }
                    let message = if action == ConfigAction::RefreshModels {
                        if self.language == crate::core::config::Language::ZhTw {
                            format!("已更新模型列表：{} 個模型", self.discovered_models.len())
                        } else {
                            format!(
                                "Model list updated: {} models",
                                self.discovered_models.len()
                            )
                        }
                    } else {
                        self.language.tr("settings_written").into()
                    };
                    self.toast = Some(Toast {
                        message,
                        is_success: true,
                    });
                }
                Err(error) if self.jobs.fail(id, error.clone()) => {
                    let msg = if self.language == crate::core::config::Language::ZhTw {
                        format!("工作失敗：{error}")
                    } else {
                        format!("Job failed: {error}")
                    };
                    self.toast = Some(Toast {
                        message: msg,
                        is_success: false,
                    });
                }
                _ => {}
            },
            Message::LaunchFinished(id, result) => match result {
                Ok(path) if self.jobs.accept(id) => {
                    self.apply_status(Some(path));
                    self.toast = None;
                    if let Some(window_id) = self.window_id {
                        self.is_hidden = true;
                        return window::set_mode(window_id, Mode::Hidden);
                    }
                }
                Err(error) if self.jobs.fail(id, error.clone()) => {
                    let msg = if self.language == crate::core::config::Language::ZhTw {
                        format!("啟動失敗：{error}")
                    } else {
                        format!("Launch failed: {error}")
                    };
                    self.toast = Some(Toast {
                        message: msg,
                        is_success: false,
                    });
                }
                _ => {}
            },
            Message::ResyncFinished(id, result) => {
                let success_msg = self.language.tr("sync_success");
                let op_msg = self.language.tr("sync");
                self.finish_unit_job(id, result, success_msg, op_msg);
            }
            Message::RestoreFinished(id, result) => match result {
                Ok(()) if self.jobs.accept(id) => {
                    self.api_key.clear();
                    self.api_key_placeholder = self.language.tr("key_enter_tip").into();
                    self.toast = Some(Toast {
                        message: self.language.tr("reset_success").into(),
                        is_success: true,
                    });
                }
                Err(error) => {
                    let op_msg = self.language.tr("reset");
                    self.finish_unit_job(id, Err(error), "", op_msg);
                }
                _ => {}
            },
            Message::StatusLoaded(path) => self.apply_status(path),
            Message::SettingsLoaded(Ok(Some(settings))) => self.apply_settings(settings.0),
            Message::SettingsLoaded(Ok(None)) => {}
            Message::SettingsLoaded(Err(error)) => {
                let msg = if self.language == crate::core::config::Language::ZhTw {
                    format!("載入設定失敗：{error}")
                } else {
                    format!("Failed to load settings: {error}")
                };
                self.toast = Some(Toast {
                    message: msg,
                    is_success: false,
                });
            }
            Message::ThemeSaved(Err(error)) => {
                let msg = if self.language == crate::core::config::Language::ZhTw {
                    format!("儲存佈景主題失敗：{error}")
                } else {
                    format!("Failed to save theme: {error}")
                };
                self.toast = Some(Toast {
                    message: msg,
                    is_success: false,
                });
            }
            Message::ThemeSaved(Ok(())) => {}
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
            Message::ModelVisibilityToggled(model, visible) => {
                if visible {
                    self.model_visibility_overrides.remove(&model);
                } else {
                    self.model_visibility_overrides.insert(model, false);
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
