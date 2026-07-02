use iced::{widget::button, Background, Border, Color, Shadow, Theme};

// ════════════════════════════════════════════════════════════════
//  色彩系統 — 深色紫藍主題
// ════════════════════════════════════════════════════════════════

pub const CLR_BG: Color = Color::from_rgb(0.098, 0.102, 0.176);
pub const CLR_CARD: Color = Color::from_rgb(0.141, 0.149, 0.227);
pub const CLR_PRIMARY: Color = Color::from_rgb(0.478, 0.408, 1.0);
pub const CLR_PRIMARY_HOVER: Color = Color::from_rgb(0.57, 0.51, 1.0);
pub const CLR_PRIMARY_PRESS: Color = Color::from_rgb(0.40, 0.34, 0.87);
pub const CLR_TEXT: Color = Color::from_rgb(0.906, 0.914, 0.961);
pub const CLR_TEXT_DIM: Color = Color::from_rgb(0.533, 0.549, 0.647);
pub const CLR_SUCCESS: Color = Color::from_rgb(0.298, 0.831, 0.494);
pub const CLR_DANGER: Color = Color::from_rgb(1.0, 0.380, 0.380);
pub const CLR_DANGER_HOVER: Color = Color::from_rgb(1.0, 0.46, 0.46);
pub const CLR_WARNING: Color = Color::from_rgb(1.0, 0.694, 0.298);
pub const CLR_BORDER: Color = Color::from_rgb(0.208, 0.220, 0.329);
pub const CLR_BTN_SEC: Color = Color::from_rgb(0.173, 0.184, 0.278);
pub const CLR_BTN_SEC_HOVER: Color = Color::from_rgb(0.216, 0.227, 0.329);
pub const CLR_SIDEBAR: Color = Color::from_rgb(0.063, 0.067, 0.122);

/// 統一的參數化按鈕樣式生成器，消除重複代碼
pub fn generic_button_style(
    active_bg: Background,
    hover_bg: Background,
    press_bg: Background,
    text_color: Color,
    border: Border,
    status: button::Status,
) -> button::Style {
    match status {
        button::Status::Active => button::Style {
            background: Some(active_bg),
            text_color,
            border,
            shadow: Shadow::default(),
            snap: false,
        },
        button::Status::Hovered => button::Style {
            background: Some(hover_bg),
            text_color,
            border,
            shadow: Shadow::default(),
            snap: false,
        },
        button::Status::Pressed => button::Style {
            background: Some(press_bg),
            text_color,
            border,
            shadow: Shadow::default(),
            snap: false,
        },
        button::Status::Disabled => button::Style {
            background: Some(active_bg),
            text_color: CLR_TEXT_DIM,
            border,
            shadow: Shadow::default(),
            snap: false,
        },
    }
}

pub fn primary_btn_style(_theme: &Theme, status: button::Status) -> button::Style {
    generic_button_style(
        Background::Color(CLR_PRIMARY),
        Background::Color(CLR_PRIMARY_HOVER),
        Background::Color(CLR_PRIMARY_PRESS),
        CLR_TEXT,
        Border {
            radius: 8.0.into(),
            ..Default::default()
        },
        status,
    )
}

pub fn secondary_btn_style(_theme: &Theme, status: button::Status) -> button::Style {
    generic_button_style(
        Background::Color(CLR_BTN_SEC),
        Background::Color(CLR_BTN_SEC_HOVER),
        Background::Color(CLR_BTN_SEC),
        CLR_TEXT,
        Border {
            radius: 8.0.into(),
            width: 1.0,
            color: CLR_BORDER,
        },
        status,
    )
}

pub fn outline_btn_style(_theme: &Theme, status: button::Status) -> button::Style {
    generic_button_style(
        Background::Color(Color::TRANSPARENT),
        Background::Color(Color::from_rgba(1.0, 0.38, 0.38, 0.1)),
        Background::Color(Color::from_rgba(1.0, 0.38, 0.38, 0.2)),
        CLR_DANGER,
        Border {
            radius: 8.0.into(),
            width: 1.0,
            color: CLR_BORDER,
        },
        status,
    )
}

pub fn danger_btn_style(_theme: &Theme, status: button::Status) -> button::Style {
    generic_button_style(
        Background::Color(CLR_DANGER),
        Background::Color(CLR_DANGER_HOVER),
        Background::Color(CLR_DANGER),
        CLR_TEXT,
        Border {
            radius: 8.0.into(),
            ..Default::default()
        },
        status,
    )
}

pub fn ghost_btn_style(_theme: &Theme, status: button::Status) -> button::Style {
    generic_button_style(
        Background::Color(Color::TRANSPARENT),
        Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.05)),
        Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.1)),
        CLR_PRIMARY,
        Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        status,
    )
}
