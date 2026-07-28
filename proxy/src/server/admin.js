    const $ = id => document.getElementById(id);
    let loadedSettings = null;
    let launchAfterSave = false;

    const translations = {
      'zh-tw': {
        'nav_connection': '連線設定',
        'nav_models': '模型設定',
        'nav_extensions': '請求最佳化與工具優化',
        'nav_optimization': '進階設定',
        'conn_title': '基本連線設定',
        'conn_provider': 'API 供應商',
        'conn_api_url': 'API URL',
        'conn_api_key': 'API Key',
        'conn_auth_scheme': '驗證方式',
        'conn_custom_path_title': '使用自訂 Claude.exe 路徑',
        'conn_custom_path_desc': '啟用後使用下方自訂的 Claude.exe 路徑',
        'conn_detected_claude': '已偵測 Claude Desktop',
        'conn_detecting': '偵測中...',
        'models_title': '模型別名與路由',
        'models_desc': '配置 Claude Desktop 對應的核心別名模型，可手動輸入或從偵測到的上游模型中選擇。',
        'models_sonnet': 'Sonnet 模型別名',
        'models_opus': 'Opus 模型別名',
        'models_haiku': 'Haiku 模型別名',
        'models_fallback': '預設保底模型',
        'models_discovered_title': '已偵測上游模型清單',
        'models_fetch_btn': '抓取模型清單',
        'models_discovered_desc': '勾選「顯示」使其呈現在 Claude Desktop 列表中；「1M」啟用 100 萬上下文支援；「預設 1M」讓第一個模型預設選用 1M。',
        'models_table_name': '模型名稱',
        'models_table_show': '顯示',
        'models_table_1m': '1M',
        'models_table_1m_default': '預設 1M',
        'models_table_effort': '推理強度',
        'models_toggle_show': '全選/取消顯示',
        'models_toggle_1m': '全選/取消 1M',
        'models_search_placeholder': '搜尋模型名稱 (例如: claude, gpt, gemini, free)...',
        'ext_title': '請求最佳化與工具優化',
        'ext_quota_title': '配額檢查攔截',
        'ext_quota_desc': '攔截 max_tokens=1 的配額與背景探測請求',
        'ext_prefix_title': '命令前綴快速檢測',
        'ext_prefix_desc': '本地解析 shell 命令，避免不必要地呼叫 LLM',
        'ext_title_skip_title': '跳過對話標題生成',
        'ext_title_skip_desc': '直接回傳固定標題 "Conversation"，加速對話開啟',
        'ext_suggest_skip_title': '跳過建議提問模式',
        'ext_suggest_skip_desc': '直接回傳空建議，減少無用 API 請求',
        'ext_filepath_title': '本機檔案路徑提取',
        'ext_filepath_desc': '由命令輸出中進行本地路徑分析',
        'api_log_title': 'API 呼叫紀錄',
        'api_log_enable_title': 'API 呼叫紀錄',
        'api_log_enable_desc': '記錄模型、狀態與耗時；不保存提示詞、回應內容或 API Key。最多保留 5 個 10 MiB 檔案。',
        'ext_web_tools_title': 'Web 網頁存取工具',
        'ext_web_tools_desc': '允許本地執行 web_search 與 web_fetch 抓取工具',
        'ext_web_fetch_schemes': 'Web Fetch 允許 URL Schemes (以逗號分隔)',
        'ext_web_fetch_private': '允許 web_fetch 存取私有網路 (Private Networks)',
        'opt_title': '進階設定',
        'opt_transport': '傳輸協定',
        'opt_transport_openai': 'OpenAI Chat 格式轉換',
        'opt_transport_anthropic': '原生 Anthropic passthrough',
        'opt_thinking': '思考模式',
        'opt_thinking_inline': 'Inline（包裝在 <antThinking> 標籤）',
        'opt_thinking_separate': 'Separate（Claude 原生 thinking 區塊）',
        'btn_launch_claude': '啟動 Claude Desktop',
        'btn_reset_mirror': '重置鏡像 Profile',
        'btn_sync_original': '從原版同步',
        'btn_save_only': '儲存',
        'btn_save_launch': '啟動 ↵',
        'toast_save_success': '設定已成功儲存！',
        'toast_save_failed': '儲存失敗: ',
        'toast_load_success': '設定載入成功！',
        'toast_load_failed': '載入失敗: ',
        'toast_launch_success': 'Claude Desktop 啟動成功，路徑: ',
        'toast_launch_failed': '儲存成功，但 Claude 啟動失敗: ',
        'toast_fetch_success': '模型清單抓取成功！',
        'toast_fetch_failed': '抓取失敗: ',
        'confirm_sync': '⚠ 確定要從官方原版 Claude Desktop 同步配置？',
        'confirm_reset': '⚠ 確定要重置鏡像 Profile 目錄？原版目錄完全不受影響。',
        'conn_detecting': '偵測中...',
        'detected_online': '已偵測 Claude Desktop',
        'detected_offline': '未偵測到安裝路徑，將使用預設路徑',
        'detected_failed': '無法偵測安裝路徑',
        'detected_offline_title': '未偵測到 Claude Desktop',
        'detected_failed_title': '無法偵測 Claude Desktop',
        'sidebar_subtitle': '設定',
        'content_title': 'FreeClaude 控制台',
        'close_title': '你的工作空間已開啟',
        'close_desc': 'Claude Desktop 正在啟動，這個瀏覽器頁面可以關閉。',
        'conn_local_proxy': '本機 Proxy',
        'placeholder_sonnet': '例如: claude-3-5-sonnet-latest',
        'placeholder_opus': '例如: claude-3-opus-latest',
        'placeholder_haiku': '例如: claude-3-5-haiku-latest',
        'placeholder_fallback': '當找不到路由時使用',
        'apiKey_saved': '•••••••••••••••• (已儲存)',
        'apiKey_not_set': '尚未設定 API Key',
        'keyStatus_saved': '✅ 已儲存金鑰',
        'keyStatus_not_set': '❌ 未儲存金鑰'
      },
      'en': {
        'nav_connection': 'Connection',
        'nav_models': 'Model Settings',
        'nav_extensions': 'Request Optimization & Tools',
        'nav_optimization': 'Advanced Settings',
        'conn_title': 'Connection Settings',
        'conn_provider': 'API Provider',
        'conn_api_url': 'API URL',
        'conn_api_key': 'API Key',
        'conn_auth_scheme': 'Auth Scheme',
        'conn_custom_path_title': 'Use custom Claude.exe path',
        'conn_custom_path_desc': 'Enable this to use the custom Claude.exe path below',
        'conn_detected_claude': 'Claude Desktop Detected',
        'conn_detecting': 'Detecting...',
        'models_title': 'Model Aliases & Routing',
        'models_desc': 'Configure core model aliases for Claude Desktop. Select or type custom ones.',
        'models_sonnet': 'Sonnet Model Alias',
        'models_opus': 'Opus Model Alias',
        'models_haiku': 'Haiku Model Alias',
        'models_fallback': 'Fallback Model',
        'models_discovered_title': 'Discovered Models',
        'models_fetch_btn': 'Fetch Models',
        'models_discovered_desc': 'Check "Show" to present in Claude; "1M" enables 1M-token context support; "Default 1M" makes the first model use 1M by default.',
        'models_table_name': 'Model Name',
        'models_table_show': 'Show',
        'models_table_1m': '1M',
        'models_table_1m_default': 'Default 1M',
        'models_table_effort': 'Reasoning Effort',
        'models_toggle_show': 'Toggle All Show',
        'models_toggle_1m': 'Toggle All 1M',
        'models_search_placeholder': 'Search model name (e.g. claude, gpt, gemini, free)...',
        'ext_title': 'Request Optimization & Tools',
        'ext_quota_title': 'Quota Mock',
        'ext_quota_desc': 'Intercept max_tokens=1 quota and background probes',
        'ext_prefix_title': 'Prefix Detection',
        'ext_prefix_desc': 'Parse shell prefixes locally to bypass LLM',
        'ext_title_skip_title': 'Skip Title Generation',
        'ext_title_skip_desc': 'Return static title "Conversation" to speed up',
        'ext_suggest_skip_title': 'Skip Suggestion Mode',
        'ext_suggest_skip_desc': 'Return empty suggestions to reduce API usage',
        'ext_filepath_title': 'Filepath Extraction',
        'ext_filepath_desc': 'Extract filepaths locally from command output',
        'api_log_title': 'API Call Logs',
        'api_log_enable_title': 'API Call Logging',
        'api_log_enable_desc': 'Logs models, status, and timing only. Prompts, responses, and API keys are excluded. Keeps at most five 10 MiB files.',
        'ext_web_tools_title': 'Web Access Tools',
        'ext_web_tools_desc': 'Enable local execution of web_search and web_fetch',
        'ext_web_fetch_schemes': 'Allowed URL Schemes (comma separated)',
        'ext_web_fetch_private': 'Allow web_fetch to access private networks',
        'opt_title': 'Advanced Settings',
        'opt_transport': 'Transport Protocol',
        'opt_transport_openai': 'OpenAI Chat Format Conversion',
        'opt_transport_anthropic': 'Native Anthropic Passthrough',
        'opt_thinking': 'Thinking Mode',
        'opt_thinking_inline': 'Inline (wrapped in <antThinking> tags)',
        'opt_thinking_separate': 'Separate (native Claude thinking blocks)',
        'btn_launch_claude': 'Launch Claude',
        'btn_reset_mirror': 'Reset Mirror',
        'btn_sync_original': 'Sync from Official',
        'btn_save_only': 'Save Only',
        'btn_save_launch': 'Launch ↵',
        'toast_save_success': 'Settings saved successfully!',
        'toast_save_failed': 'Save failed: ',
        'toast_load_success': 'Settings loaded successfully!',
        'toast_load_failed': 'Load failed: ',
        'toast_launch_success': 'Claude Desktop launched, path: ',
        'toast_launch_failed': 'Saved, but failed to launch Claude: ',
        'toast_fetch_success': 'Model list fetched successfully!',
        'toast_fetch_failed': 'Fetch failed: ',
        'confirm_sync': '⚠ Are you sure you want to sync settings from original Claude?',
        'confirm_reset': '⚠ Are you sure you want to reset mirror Profile? Original profile will not be affected.',
        'detected_online': 'Claude Desktop Detected',
        'detected_offline': 'Claude Desktop not detected, using default path',
        'detected_failed': 'Failed to detect Claude Desktop path',
        'detected_offline_title': 'Claude Desktop Not Detected',
        'detected_failed_title': 'Claude Desktop Detection Failed',
        'sidebar_subtitle': 'Console',
        'content_title': 'FreeClaude Console',
        'close_title': 'Your workspace is open',
        'close_desc': 'Claude Desktop is starting, so you can close this browser page.',
        'conn_local_proxy': 'Local Proxy',
        'placeholder_sonnet': 'e.g. claude-3-5-sonnet-latest',
        'placeholder_opus': 'e.g. claude-3-opus-latest',
        'placeholder_haiku': 'e.g. claude-3-5-haiku-latest',
        'placeholder_fallback': 'Used when no route matches',
        'apiKey_saved': '•••••••••••••••• (Saved)',
        'apiKey_not_set': 'API Key not set',
        'keyStatus_saved': '✅ Saved',
        'keyStatus_not_set': '❌ Not set'
      }
    };

    function applyLanguage(lang) {
      const dict = translations[lang] || translations['zh-tw'];
      document.querySelectorAll('[data-i18n]').forEach(el => {
        const key = el.dataset.i18n;
        if (dict[key]) {
          el.textContent = dict[key];
        }
      });
      document.querySelectorAll('[data-i18n-placeholder]').forEach(el => {
        const key = el.dataset.i18nPlaceholder;
        if (dict[key]) {
          el.placeholder = dict[key];
        }
      });
      if (loadedSettings) {
        $('apiKey').placeholder = loadedSettings.hasApiKey ? dict['apiKey_saved'] : dict['apiKey_not_set'];
        $('keyStatus').textContent = loadedSettings.hasApiKey ? dict['keyStatus_saved'] : dict['keyStatus_not_set'];
      }
      document.title = dict['title'] || 'FreeClaude Admin Dashboard';
      document.documentElement.lang = lang === 'en' ? 'en' : 'zh-TW';
    }

    function t(key, param = '') {
      const lang = $('language').value || 'zh-tw';
      const dict = translations[lang] || translations['zh-tw'];
      let text = dict[key] || key;
      if (param) {
        text += param;
      }
      return text;
    }

    const providerPresets = {
      nvidia: { baseUrl: 'https://integrate.api.nvidia.com/v1', authScheme: 'bearer' },
      openrouter: { baseUrl: 'https://openrouter.ai/api/v1', authScheme: 'bearer' },
      gemini: { baseUrl: 'https://generativelanguage.googleapis.com/v1beta/openai', authScheme: 'bearer' },
      deepseek: { baseUrl: 'https://api.deepseek.com', authScheme: 'bearer' },
      groq: { baseUrl: 'https://api.groq.com/openai/v1', authScheme: 'bearer' },
      grok: { baseUrl: 'https://api.x.ai/v1', authScheme: 'bearer' },
      zai: { baseUrl: 'https://api.z.ai/api/paas/v4', authScheme: 'bearer' },
      kimi: { baseUrl: 'https://api.moonshot.ai/v1', authScheme: 'bearer' },
      minimax: { baseUrl: 'https://api.minimax.io/v1', authScheme: 'bearer' },
      qwen: { baseUrl: 'https://dashscope-intl.aliyuncs.com/compatible-mode/v1', authScheme: 'bearer' }
    };

    function selectProviderForBaseUrl(baseUrl) {
      const normalized = (baseUrl || '').replace(/\/$/, '');
      const provider = Object.entries(providerPresets)
        .find(([, preset]) => preset.baseUrl === normalized)?.[0] || 'custom';
      $('apiProvider').value = provider;
    }

    $('apiProvider').addEventListener('change', () => {
      const preset = providerPresets[$('apiProvider').value];
      if (!preset) return;
      $('baseUrl').value = preset.baseUrl;
      $('authScheme').value = preset.authScheme;
    });

    // Helper functions for Toast
    function showToast(message, type = 'success') {
      const container = $('toastContainer');
      const toast = document.createElement('div');
      toast.className = `toast ${type}`;

      let icon = '';
      if (type === 'success') {
        icon = `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"/><polyline points="22 4 12 14.01 9 11.01"/></svg>`;
      } else {
        icon = `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><line x1="15" y1="9" x2="9" y2="15"/><line x1="9" y1="9" x2="15" y2="15"/></svg>`;
      }

      toast.innerHTML = `${icon}<span>${message}</span>`;
      container.appendChild(toast);

      setTimeout(() => toast.classList.add('show'), 10);

      setTimeout(() => {
        toast.classList.remove('show');
        setTimeout(() => toast.remove(), 300);
      }, 3500);
    }

    function showLoading(show) {
      $('loadingOverlay').style.display = show ? 'flex' : 'none';
    }

    const headers = () => ({});

    async function request(path, options = {}) {
      const r = await fetch(path, {
        ...options,
        headers: {
          ...headers(),
          ...(options.headers || {})
        }
      });
      const b = await r.json();
      if (!r.ok) throw new Error(b.error || r.statusText);
      return b;
    }

    // Toggle Web Fetch options when Web Tools Intercept checked
    $('enableWebServerTools').addEventListener('change', (e) => {
      if (e.target.checked) {
        $('webToolsSettings').classList.remove('hidden');
      } else {
        $('webToolsSettings').classList.add('hidden');
      }
    });

    // Theme switching logic
    function applyTheme(theme) {
      const root = document.documentElement;
      if (theme === 'system') {
        const isDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
        root.setAttribute('data-theme', isDark ? 'dark' : 'light');
      } else {
        root.setAttribute('data-theme', theme);
      }

      document.querySelectorAll('.theme-btn').forEach(btn => {
        if (btn.dataset.theme === theme) {
          btn.classList.add('active');
        } else {
          btn.classList.remove('active');
        }
      });

      localStorage.setItem('theme', theme);
      if ($('themeMode')) {
        $('themeMode').value = theme;
      }
    }

    document.querySelectorAll('.theme-btn').forEach(btn => {
      btn.addEventListener('click', () => {
        applyTheme(btn.dataset.theme);
      });
    });

    window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
      const currentTheme = localStorage.getItem('theme') || 'system';
      if (currentTheme === 'system') {
        applyTheme('system');
      }
    });

    // Sidebar navigation logic
    document.querySelectorAll('.nav-item').forEach(item => {
      item.addEventListener('click', (e) => {
        e.preventDefault();
        const tabId = item.dataset.tab;

        document.querySelectorAll('.nav-item').forEach(i => i.classList.remove('active'));
        item.classList.add('active');

        document.querySelectorAll('.tab-section').forEach(sec => {
          if (sec.id === `tab-${tabId}`) {
            sec.classList.remove('hidden');
          } else {
            sec.classList.add('hidden');
          }
        });
      });
    });

    // Load Settings
    async function load() {
      showLoading(true);
      $('detectedClaudeDot').className = 'status-dot offline';
      $('detectedClaudeStatus').setAttribute('data-i18n', 'conn_detecting');
      $('detectedClaudeStatus').textContent = t('conn_detecting');
      $('detectedClaudePath').setAttribute('data-i18n', 'conn_detecting');
      $('detectedClaudePath').textContent = t('conn_detecting');
      try {
        const [settings, status] = await Promise.all([
          request('/settings'),
          request('/status')
        ]);

        loadedSettings = settings;

        $('authWrapper').classList.add('hidden');
        $('mainLayout').classList.remove('hidden');
        $('appContainer').classList.remove('unauthorized');

        $('activePort').textContent = '127.0.0.1 : ' + (status.proxy.port || '3000');

        $('baseUrl').value = settings.baseUrl || '';
        $('authScheme').value = settings.authScheme || 'bearer';
        selectProviderForBaseUrl(settings.baseUrl);
        $('apiKey').placeholder = settings.hasApiKey ? t('apiKey_saved') : t('apiKey_not_set');
        $('keyStatus').textContent = settings.hasApiKey ? t('keyStatus_saved') : t('keyStatus_not_set');
        $('keyStatus').style.color = settings.hasApiKey ? '#10b981' : '#f59e0b';

        $('customClaudePath').value = settings.customClaudePath || '';
        $('useCustomClaudePath').checked = Boolean(settings.customClaudePath);
        syncCustomClaudePathUI();

        $('realModelSonnet').value = settings.realModelSonnet || '';
        $('realModelOpus').value = settings.realModelOpus || '';
        $('realModelHaiku').value = settings.realModelHaiku || '';
        $('realModel').value = settings.realModel || '';

        const dl = $('modelSuggestions');
        dl.innerHTML = '';
        if (settings.discoveredModels) {
          settings.discoveredModels.forEach(m => {
            const opt = document.createElement('option');
            opt.value = m;
            dl.appendChild(opt);
          });
        }

        $('transportType').value = settings.transportType || 'openai_chat';
        const reasoningReplayMode = ['inline', 'separate'].includes(settings.reasoningReplayMode)
          ? settings.reasoningReplayMode
          : 'separate';
        $('reasoningReplayMode').value = reasoningReplayMode;

        $('enableQuotaCheckMock').checked = settings.enableQuotaCheckMock !== false;
        $('enablePrefixDetection').checked = settings.enablePrefixDetection !== false;
        $('enableTitleGenerationSkip').checked = settings.enableTitleGenerationSkip !== false;
        $('enableSuggestionModeSkip').checked = settings.enableSuggestionModeSkip !== false;
        $('enableFilepathExtractionMock').checked = settings.enableFilepathExtractionMock !== false;
        $('enableApiCallLogging').checked = settings.enableApiCallLogging === true;

        $('enableWebServerTools').checked = settings.enableWebServerTools === true;
        if (settings.enableWebServerTools) {
          $('webToolsSettings').classList.remove('hidden');
        } else {
          $('webToolsSettings').classList.add('hidden');
        }

        $('webFetchAllowedSchemes').value = settings.webFetchAllowedSchemes || 'http,https';
        $('webFetchAllowPrivateNetworks').checked = settings.webFetchAllowPrivateNetworks === true;

        const theme = localStorage.getItem('theme') || settings.themeMode || 'system';
        localStorage.setItem('theme', theme);
        applyTheme(theme);

        $('language').value = settings.language || 'zh-tw';
        applyLanguage($('language').value);
        renderModelsTable(settings);

        // Detect Claude Path via RPC
        try {
          const detectRes = await request('/rpc', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ method: 'DetectClaude' })
          });
          if (detectRes && detectRes.result && detectRes.result.path) {
            $('detectedClaudeDot').className = 'status-dot online';
            $('detectedClaudeStatus').setAttribute('data-i18n', 'detected_online');
            $('detectedClaudeStatus').textContent = t('detected_online');
            $('detectedClaudePath').removeAttribute('data-i18n');
            $('detectedClaudePath').textContent = detectRes.result.path;
          } else {
            $('detectedClaudeDot').className = 'status-dot offline';
            $('detectedClaudeStatus').setAttribute('data-i18n', 'detected_offline_title');
            $('detectedClaudeStatus').textContent = t('detected_offline_title');
            $('detectedClaudePath').setAttribute('data-i18n', 'detected_offline');
            $('detectedClaudePath').textContent = t('detected_offline');
          }
        } catch (err) {
          $('detectedClaudeDot').className = 'status-dot failed';
          $('detectedClaudeStatus').setAttribute('data-i18n', 'detected_failed_title');
          $('detectedClaudeStatus').textContent = t('detected_failed_title');
          $('detectedClaudePath').setAttribute('data-i18n', 'detected_failed');
          $('detectedClaudePath').textContent = t('detected_failed');
        }

        showToast(t('toast_load_success'));
      } catch (e) {
        $('authWrapper').classList.add('hidden');
        $('mainLayout').classList.remove('hidden');
        $('appContainer').classList.remove('unauthorized');
        showToast(t('toast_load_failed') + e.message, 'error');
      } finally {
        showLoading(false);
      }
    }

    let currentSort = { key: null, asc: true };
    let searchQuery = '';

    function syncCustomClaudePathUI() {
      const enabled = $('useCustomClaudePath').checked;
      $('customClaudePath').disabled = !enabled;
      $('customClaudePath').style.opacity = enabled ? '1' : '0.5';
    }

    $('useCustomClaudePath').onchange = syncCustomClaudePathUI;

    function syncStateFromDOM() {
      if (!loadedSettings) return;
      if (!loadedSettings.modelVisibilityOverrides) loadedSettings.modelVisibilityOverrides = {};
      if (!loadedSettings.model1mOverrides) loadedSettings.model1mOverrides = {};
      if (!loadedSettings.model1mPreferOverrides) loadedSettings.model1mPreferOverrides = {};
      if (!loadedSettings.modelReasoningOverrides) loadedSettings.modelReasoningOverrides = {};

      document.querySelectorAll('.model-visibility').forEach(el => {
        loadedSettings.modelVisibilityOverrides[el.dataset.model] = el.checked;
      });
      document.querySelectorAll('.model-1m').forEach(el => {
        loadedSettings.model1mOverrides[el.dataset.model] = el.checked;
      });
      document.querySelectorAll('.model-1m-prefer').forEach(el => {
        const supports1m = loadedSettings.model1mOverrides[el.dataset.model] === true;
        loadedSettings.model1mPreferOverrides[el.dataset.model] = supports1m && el.checked;
      });
      document.querySelectorAll('.model-effort').forEach(el => {
        loadedSettings.modelReasoningOverrides[el.dataset.model] = el.value;
      });
    }

    function renderModelsTable(settings) {
      const tbody = $('modelsTableBody');
      tbody.innerHTML = '';

      let models = (settings.discoveredModels || []).slice();
      const lang = $('language').value || 'zh-tw';

      if (models.length === 0) {
        tbody.innerHTML = `<tr><td colspan="5">${t('no_models_yet')}</td></tr>`;
        return;
      }

      if (searchQuery) {
        models = models.filter(m => m.toLowerCase().includes(searchQuery));
      }

      if (models.length === 0) {
        tbody.innerHTML = `<tr><td colspan="5">未找到符合關鍵字 "${searchQuery}" 的模型</td></tr>`;
        return;
      }

      if (currentSort.key) {
        const effortRank = { '': 0, 'none': 1, 'high': 2, 'max': 3 };
        models.sort((a, b) => {
          let valA, valB;
          if (currentSort.key === 'name') {
            valA = a.toLowerCase();
            valB = b.toLowerCase();
          } else if (currentSort.key === 'show') {
            valA = (settings.modelVisibilityOverrides && settings.modelVisibilityOverrides[a] !== false) ? 1 : 0;
            valB = (settings.modelVisibilityOverrides && settings.modelVisibilityOverrides[b] !== false) ? 1 : 0;
          } else if (currentSort.key === '1m') {
            valA = (settings.model1mOverrides && settings.model1mOverrides[a] === true) ? 1 : 0;
            valB = (settings.model1mOverrides && settings.model1mOverrides[b] === true) ? 1 : 0;
          } else if (currentSort.key === '1m-default') {
            valA = (settings.model1mPreferOverrides && settings.model1mPreferOverrides[a] === true) ? 1 : 0;
            valB = (settings.model1mPreferOverrides && settings.model1mPreferOverrides[b] === true) ? 1 : 0;
          } else if (currentSort.key === 'effort') {
            const effA = (settings.modelReasoningOverrides && settings.modelReasoningOverrides[a]) || '';
            const effB = (settings.modelReasoningOverrides && settings.modelReasoningOverrides[b]) || '';
            valA = effortRank[effA] ?? 0;
            valB = effortRank[effB] ?? 0;
          }
          if (valA < valB) return currentSort.asc ? -1 : 1;
          if (valA > valB) return currentSort.asc ? 1 : -1;
          return 0;
        });
      }

      const optDefault = lang === 'en' ? 'Default' : '預設';
      const optNone = lang === 'en' ? 'None' : '無';
      const optHigh = lang === 'en' ? 'High' : '高';
      const optMax = lang === 'en' ? 'Max' : '最高';

      models.forEach(model => {
        const tr = document.createElement('tr');

        const isVisible = settings.modelVisibilityOverrides && settings.modelVisibilityOverrides[model] !== false;
        const is1m = settings.model1mOverrides && settings.model1mOverrides[model] === true;
        const is1mPrefer = is1m && settings.model1mPreferOverrides && settings.model1mPreferOverrides[model] === true;
        const effort = (settings.modelReasoningOverrides && settings.modelReasoningOverrides[model]) || '';

        tr.innerHTML = `
          <td class="admin-inline-style-24">${model}</td>
          <td class="admin-inline-style-25">
            <input type="checkbox" class="model-visibility" data-model="${model}" ${isVisible ? 'checked' : ''} aria-label="${model} 顯示狀態">
          </td>
          <td class="admin-inline-style-25">
            <input type="checkbox" class="model-1m" data-model="${model}" ${is1m ? 'checked' : ''} aria-label="${model} 1M Context 支援">
          </td>
          <td class="admin-inline-style-25">
            <input type="checkbox" class="model-1m-prefer" data-model="${model}" ${is1mPrefer ? 'checked' : ''} ${is1m ? '' : 'disabled'} aria-label="${model} 預設使用 1M">
          </td>
          <td>
            <div class="select-wrapper">
              <select class="model-effort" data-model="${model}" aria-label="${model} 思考上限設定">
                <option value="" ${effort === '' ? 'selected' : ''}>${optDefault}</option>
                <option value="none" ${effort === 'none' ? 'selected' : ''}>${optNone}</option>
                <option value="high" ${effort === 'high' ? 'selected' : ''}>${optHigh}</option>
                <option value="max" ${effort === 'max' ? 'selected' : ''}>${optMax}</option>
              </select>
            </div>
          </td>
        `;
        tbody.appendChild(tr);
      });

      document.querySelectorAll('.model-1m').forEach(el => {
        el.onchange = () => {
          const prefer = Array.from(document.querySelectorAll('.model-1m-prefer'))
            .find(item => item.dataset.model === el.dataset.model);
          if (!prefer) return;
          prefer.disabled = !el.checked;
          // 啟用 1M 變體時，同步將它設為預設選項，避免只輸出
          // `supports1m` 而遺漏 Claude Desktop 所需的 `prefer1m`。
          prefer.checked = el.checked;
        };
      });

      ['name', 'show', '1m', '1m-default', 'effort'].forEach(k => {
        const iconEl = $(`sort-icon-${k}`);
        if (iconEl) {
          if (currentSort.key === k) {
            iconEl.textContent = currentSort.asc ? '▲' : '▼';
          } else {
            iconEl.textContent = '↕';
          }
        }
      });
    }

    if ($('modelSearchInput')) {
      $('modelSearchInput').oninput = (e) => {
        searchQuery = e.target.value.trim().toLowerCase();
        syncStateFromDOM();
        if (loadedSettings) {
          renderModelsTable(loadedSettings);
        }
      };
      $('modelSearchInput').onkeydown = (e) => {
        if (e.key === 'Enter') {
          e.preventDefault();
        }
      };
    }

    if ($('toggleAllShowBtn')) {
      $('toggleAllShowBtn').onclick = () => {
        syncStateFromDOM();
        const checkboxes = document.querySelectorAll('.model-visibility');
        if (checkboxes.length === 0) return;
        const allChecked = Array.from(checkboxes).every(cb => cb.checked);
        checkboxes.forEach(cb => {
          cb.checked = !allChecked;
          if (loadedSettings && loadedSettings.modelVisibilityOverrides) {
            loadedSettings.modelVisibilityOverrides[cb.dataset.model] = !allChecked;
          }
        });
      };
    }

    if ($('toggleAll1mBtn')) {
      $('toggleAll1mBtn').onclick = () => {
        syncStateFromDOM();
        const checkboxes = document.querySelectorAll('.model-1m');
        if (checkboxes.length === 0) return;
        const allChecked = Array.from(checkboxes).every(cb => cb.checked);
        checkboxes.forEach(cb => {
          cb.checked = !allChecked;
          if (loadedSettings && loadedSettings.model1mOverrides) {
            loadedSettings.model1mOverrides[cb.dataset.model] = !allChecked;
          }
          const prefer = Array.from(document.querySelectorAll('.model-1m-prefer'))
            .find(item => item.dataset.model === cb.dataset.model);
          if (prefer) {
            prefer.disabled = allChecked;
            prefer.checked = !allChecked;
            if (loadedSettings && loadedSettings.model1mPreferOverrides) {
              loadedSettings.model1mPreferOverrides[cb.dataset.model] = !allChecked;
            }
          }
        });
      };
    }

    document.querySelectorAll('.sortable-th').forEach(th => {
      th.onclick = () => {
        const sortKey = th.dataset.sort;
        if (currentSort.key === sortKey) {
          currentSort.asc = !currentSort.asc;
        } else {
          currentSort.key = sortKey;
          currentSort.asc = true;
        }
        syncStateFromDOM();
        if (loadedSettings) {
          renderModelsTable(loadedSettings);
        }
      };
    });

    $('loadBtn').onclick = load;
    // Save Logic
    $('saveAndLaunchBtn').onclick = () => {
      launchAfterSave = true;
      $('settingsForm').requestSubmit();
    };
    $('saveOnlyBtn').onclick = () => {
      launchAfterSave = false;
      $('settingsForm').requestSubmit();
    };

    $('settingsForm').onkeydown = (e) => {
      if (e.key === 'Enter') {
        e.preventDefault();
      }
    };

    $('settingsForm').onsubmit = async (e) => {
      e.preventDefault();

      showLoading(true);
      try {
        const modelVisibilityOverrides = {};
        document.querySelectorAll('.model-visibility').forEach(el => {
          modelVisibilityOverrides[el.dataset.model] = el.checked;
        });

        const model1mOverrides = {};
        document.querySelectorAll('.model-1m').forEach(el => {
          model1mOverrides[el.dataset.model] = el.checked;
        });

        const model1mPreferOverrides = {};
        document.querySelectorAll('.model-1m-prefer').forEach(el => {
          model1mPreferOverrides[el.dataset.model] =
            model1mOverrides[el.dataset.model] === true && el.checked;
        });

        const modelReasoningOverrides = {};
        document.querySelectorAll('.model-effort').forEach(el => {
          modelReasoningOverrides[el.dataset.model] = el.value;
        });

        const payload = {
          baseUrl: $('baseUrl').value.trim(),
          authScheme: $('authScheme').value,
          apiKey: $('apiKey').value.trim() || null,
          customClaudePath: $('useCustomClaudePath').checked
            ? ($('customClaudePath').value.trim() || null)
            : null,

          realModelSonnet: $('realModelSonnet').value.trim() || null,
          realModelOpus: $('realModelOpus').value.trim() || null,
          realModelHaiku: $('realModelHaiku').value.trim() || null,
          realModel: $('realModel').value.trim() || null,

          modelVisibilityOverrides,
          model1mOverrides,
          model1mPreferOverrides,
          modelReasoningOverrides,

          transportType: $('transportType').value,
          reasoningReplayMode: $('reasoningReplayMode').value,

          enableQuotaCheckMock: $('enableQuotaCheckMock').checked,
          enablePrefixDetection: $('enablePrefixDetection').checked,
          enableTitleGenerationSkip: $('enableTitleGenerationSkip').checked,
          enableSuggestionModeSkip: $('enableSuggestionModeSkip').checked,
          enableFilepathExtractionMock: $('enableFilepathExtractionMock').checked,
          enableApiCallLogging: $('enableApiCallLogging').checked,

          enableWebServerTools: $('enableWebServerTools').checked,
          webFetchAllowedSchemes: $('webFetchAllowedSchemes').value.trim(),
          webFetchAllowPrivateNetworks: $('webFetchAllowPrivateNetworks').checked,

          themeMode: localStorage.getItem('theme') || 'system',
          language: $('language').value
        };

        await request('/settings', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(payload)
        });

        $('apiKey').value = '';
        showToast(t('toast_save_success'));

        const shouldLaunch = launchAfterSave;
        launchAfterSave = false;
        if (shouldLaunch) {
          try {
            const launchRes = await request('/rpc', {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({ method: 'LaunchClaude' })
            });
            showToast(t('toast_launch_success') + launchRes.result.path);
            setTimeout(() => {
              // 瀏覽器若允許便自動關閉；若遭安全政策阻擋，繼續顯示手動關閉提示。
              window.close();
              document.body.innerHTML = `
                <div class="admin-inline-style-26">
                  <img class="admin-inline-style-27" src="/assets/icon.png" alt="FreeClaudeDesktop 圖標" />
                  <h1 class="admin-inline-style-28">${t('close_title')}</h1>
                  <p class="admin-inline-style-29">${t('close_desc')}</p>
                </div>
              `;
            }, 1000);
            return;
          } catch (launchErr) {
            showToast(t('toast_launch_failed') + launchErr.message, 'error');
          }
        }

        await load();
      } catch (e) {
        showToast(t('toast_save_failed') + e.message, 'error');
      } finally {
        showLoading(false);
      }
    };

    // RPC Actions

    $('resetMirrorBtn').onclick = async () => {
      if (!confirm(t('confirm_reset'))) return;
      showLoading(true);
      try {
        await request('/rpc', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ method: 'ResetMirrorProfile' })
        });
        showToast(t('toast_save_success'));
        await load();
      } catch(e) {
        showToast(t('toast_save_failed') + e.message, 'error');
      } finally {
        showLoading(false);
      }
    };

    $('syncOfficialBtn').onclick = async () => {
      if (!confirm(t('confirm_sync'))) return;
      showLoading(true);
      try {
        await request('/rpc', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ method: 'SyncFromOfficial' })
        });
        showToast(t('toast_save_success'));
        await load();
      } catch(e) {
        showToast(t('toast_save_failed') + e.message, 'error');
      } finally {
        showLoading(false);
      }
    };

    $('fetchModelsBtn').onclick = async () => {
      showLoading(true);
      try {
        await request('/rpc', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ method: 'FetchModels' })
        });
        showToast(t('toast_fetch_success'));
        await load();
      } catch (e) {
        showToast(t('toast_fetch_failed') + e.message, 'error');
      } finally {
        showLoading(false);
      }
    };

    $('language').onchange = () => {
      const val = $('language').value;
      applyLanguage(val);
      if (loadedSettings) {
        renderModelsTable(loadedSettings);
      }
    };

    // Theme initialization
    (function() {
      const savedTheme = localStorage.getItem('theme') || 'system';
      applyTheme(savedTheme);
      load();
    })();

// 將原本的 inline onclick 改為集中事件綁定。
document.querySelectorAll('[data-toggle-target]').forEach(label => {
  label.addEventListener('click', () => {
    document.getElementById(label.dataset.toggleTarget)?.click();
  });
});
