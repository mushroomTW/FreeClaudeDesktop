use crate::app::ThemeMode;
use iced::{
    overlay::menu,
    widget::{button, checkbox, pick_list, text_input},
    Background, Border, Color, Shadow,
};

// ════════════════════════════════════════════════════════════════
//  色彩系統 — 動態 ColorPalette
// ════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy)]
pub struct ColorPalette {
    pub bg: Color,
    pub card: Color,
    pub sidebar: Color,
    pub text: Color,
    pub text_dim: Color,
    pub primary: Color,
    pub primary_hover: Color,
    pub primary_press: Color,
    pub primary_text: Color,
    pub border: Color,
    pub btn_sec: Color,
    pub btn_sec_hover: Color,
    pub success: Color,
    pub danger: Color,
    pub danger_hover: Color,
    pub warning: Color,
    pub menu_selected_bg: Color,
    pub input_bg: Color,
    pub segmented_bg: Color,
    pub segmented_active_bg: Color,
}

impl ColorPalette {
    pub fn for_mode(mode: ThemeMode) -> Self {
        let effective = match mode {
            ThemeMode::System => {
                if crate::platform::is_system_dark_mode() {
                    ThemeMode::Dark
                } else {
                    ThemeMode::Light
                }
            }
            other => other,
        };

        match effective {
            ThemeMode::Light | ThemeMode::System => Self {
                bg: Color::from_rgb(0.980, 0.969, 0.949), // 暖奶白/沙色 #FAF7F2
                card: Color::WHITE,                       // 純白卡片 #FFFFFF
                sidebar: Color::from_rgb(0.941, 0.925, 0.882), // 柔暖灰沙色 #F0ECE1
                text: Color::from_rgb(0.133, 0.133, 0.125), // 深炭暖灰 #222220
                text_dim: Color::from_rgb(0.420, 0.400, 0.369), // 暖灰褐 #6B665E
                primary: Color::from_rgb(0.855, 0.467, 0.337), // Claude 標誌赤陶柿橘 #DA7756
                primary_hover: Color::from_rgb(0.898, 0.533, 0.412), // #E58869
                primary_press: Color::from_rgb(0.769, 0.392, 0.271), // #C46445
                primary_text: Color::WHITE,
                border: Color::from_rgb(0.886, 0.867, 0.835), // 柔暖邊線 #E2DDD5
                btn_sec: Color::from_rgb(0.961, 0.949, 0.922), // 米白按鈕 #F5F2EC
                btn_sec_hover: Color::from_rgb(0.910, 0.890, 0.855), // #E8E3DA
                success: Color::from_rgb(0.310, 0.522, 0.349), // 暖綠色 #4F8559
                danger: Color::from_rgb(0.788, 0.290, 0.161), // 紅橘色 #C94A29
                danger_hover: Color::from_rgb(0.839, 0.369, 0.243), // #D65E3E
                warning: Color::from_rgb(0.710, 0.494, 0.141), // 焦糖黃 #B57E24
                menu_selected_bg: Color::from_rgb(0.988, 0.922, 0.902), // 淺暖珊瑚底 #FCEBE6
                input_bg: Color::WHITE,
                segmented_bg: Color::from_rgb(0.980, 0.969, 0.949), // Claude 溫暖米白底 #FAF7F2
                segmented_active_bg: Color::from_rgb(0.855, 0.467, 0.337), // Claude 標誌赤陶柿橘 #DA7756
            },
            ThemeMode::Dark => Self {
                bg: Color::from_rgb(0.106, 0.098, 0.090), // 暖炭灰石墨底 #1B1917
                card: Color::from_rgb(0.149, 0.141, 0.130), // 暖灰卡片面板 #262421
                sidebar: Color::from_rgb(0.078, 0.075, 0.071), // 沉穩暖黑側邊欄 #141312
                text: Color::from_rgb(0.925, 0.902, 0.875), // 暖柔灰白字 #ECE6DF
                text_dim: Color::from_rgb(0.631, 0.604, 0.561), // 暖中灰 #A19A8F
                primary: Color::from_rgb(0.855, 0.467, 0.337), // Claude 標誌赤陶柿橘 #DA7756
                primary_hover: Color::from_rgb(0.898, 0.533, 0.412), // #E58869
                primary_press: Color::from_rgb(0.769, 0.392, 0.271), // #C46445
                primary_text: Color::WHITE,
                border: Color::from_rgb(0.239, 0.220, 0.200), // 暖暗灰邊線 #3D3833
                btn_sec: Color::from_rgb(0.169, 0.157, 0.141), // 深暖灰按鈕 #2B2824
                btn_sec_hover: Color::from_rgb(0.212, 0.196, 0.176), // #36322D
                success: Color::from_rgb(0.353, 0.659, 0.424), // 柔暖綠 #5AA86C
                danger: Color::from_rgb(0.878, 0.388, 0.322), // 暖柿紅 #E06352
                danger_hover: Color::from_rgb(0.922, 0.459, 0.396), // #EB7565
                warning: Color::from_rgb(0.898, 0.663, 0.235), // 焦糖暖黃 #E5A93C
                menu_selected_bg: Color::from_rgb(0.239, 0.153, 0.114), // 深赤陶暗底 #3D271D
                input_bg: Color::from_rgb(0.122, 0.114, 0.102), // #1F1D1A
                segmented_bg: Color::from_rgb(0.122, 0.114, 0.102), // 深暖灰膠囊底 #1F1D1A
                segmented_active_bg: Color::from_rgb(0.855, 0.467, 0.337), // Claude 標誌赤陶柿橘 #DA7756
            },
        }
    }
}

// 相容常數（預設 Light 色彩）
pub const CLR_BG: Color = Color::from_rgb(0.980, 0.969, 0.949);
pub const CLR_CARD: Color = Color::WHITE;
pub const CLR_PRIMARY: Color = Color::from_rgb(0.855, 0.467, 0.337);
pub const CLR_PRIMARY_HOVER: Color = Color::from_rgb(0.898, 0.533, 0.412);
pub const CLR_PRIMARY_PRESS: Color = Color::from_rgb(0.769, 0.392, 0.271);
pub const CLR_TEXT: Color = Color::from_rgb(0.133, 0.133, 0.125);
pub const CLR_TEXT_DIM: Color = Color::from_rgb(0.420, 0.400, 0.369);
pub const CLR_SUCCESS: Color = Color::from_rgb(0.310, 0.522, 0.349);
pub const CLR_DANGER: Color = Color::from_rgb(0.788, 0.290, 0.161);
pub const CLR_DANGER_HOVER: Color = Color::from_rgb(0.839, 0.369, 0.243);
pub const CLR_WARNING: Color = Color::from_rgb(0.710, 0.494, 0.141);
pub const CLR_BORDER: Color = Color::from_rgb(0.886, 0.867, 0.835);
pub const CLR_BTN_SEC: Color = Color::from_rgb(0.961, 0.949, 0.922);
pub const CLR_BTN_SEC_HOVER: Color = Color::from_rgb(0.910, 0.890, 0.855);
pub const CLR_SIDEBAR: Color = Color::from_rgb(0.941, 0.925, 0.882);

/// 統一的參數化按鈕樣式生成器
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
            text_color: Color::from_rgb(0.6, 0.6, 0.6),
            border,
            shadow: Shadow::default(),
            snap: false,
        },
    }
}

pub fn primary_btn_style(palette: ColorPalette, status: button::Status) -> button::Style {
    generic_button_style(
        Background::Color(palette.primary),
        Background::Color(palette.primary_hover),
        Background::Color(palette.primary_press),
        palette.primary_text,
        Border {
            radius: 8.0.into(),
            ..Default::default()
        },
        status,
    )
}

pub fn secondary_btn_style(palette: ColorPalette, status: button::Status) -> button::Style {
    generic_button_style(
        Background::Color(palette.btn_sec),
        Background::Color(palette.btn_sec_hover),
        Background::Color(palette.btn_sec),
        palette.text,
        Border {
            radius: 8.0.into(),
            width: 1.0,
            color: palette.border,
        },
        status,
    )
}

pub fn outline_btn_style(palette: ColorPalette, status: button::Status) -> button::Style {
    generic_button_style(
        Background::Color(Color::TRANSPARENT),
        Background::Color(Color::from_rgba(
            palette.danger.r,
            palette.danger.g,
            palette.danger.b,
            0.1,
        )),
        Background::Color(Color::from_rgba(
            palette.danger.r,
            palette.danger.g,
            palette.danger.b,
            0.2,
        )),
        palette.danger,
        Border {
            radius: 8.0.into(),
            width: 1.0,
            color: palette.border,
        },
        status,
    )
}

pub fn danger_btn_style(palette: ColorPalette, status: button::Status) -> button::Style {
    generic_button_style(
        Background::Color(palette.danger),
        Background::Color(palette.danger_hover),
        Background::Color(palette.danger),
        Color::WHITE,
        Border {
            radius: 8.0.into(),
            ..Default::default()
        },
        status,
    )
}

pub fn ghost_btn_style(palette: ColorPalette, status: button::Status) -> button::Style {
    generic_button_style(
        Background::Color(Color::TRANSPARENT),
        Background::Color(Color::from_rgba(
            palette.primary.r,
            palette.primary.g,
            palette.primary.b,
            0.1,
        )),
        Background::Color(Color::from_rgba(
            palette.primary.r,
            palette.primary.g,
            palette.primary.b,
            0.2,
        )),
        palette.primary,
        Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        status,
    )
}

/// 分段切換按鈕 (Segmented Control Button) 樣式
pub fn segmented_button_style(
    palette: ColorPalette,
    is_active: bool,
    status: button::Status,
) -> button::Style {
    if is_active {
        button::Style {
            background: Some(Background::Color(palette.segmented_active_bg)),
            text_color: Color::WHITE,
            border: Border {
                radius: 8.0.into(),
                width: 0.0,
                color: Color::TRANSPARENT,
            },
            shadow: Shadow {
                color: Color::from_rgba(
                    palette.segmented_active_bg.r,
                    palette.segmented_active_bg.g,
                    palette.segmented_active_bg.b,
                    0.35,
                ),
                offset: iced::Vector::new(0.0, 1.5),
                blur_radius: 5.0,
            },
            snap: false,
        }
    } else {
        match status {
            button::Status::Active => button::Style {
                background: Some(Background::Color(Color::TRANSPARENT)),
                text_color: palette.text_dim,
                border: Border {
                    radius: 8.0.into(),
                    ..Default::default()
                },
                shadow: Shadow::default(),
                snap: false,
            },
            button::Status::Hovered => button::Style {
                background: Some(Background::Color(palette.btn_sec_hover)),
                text_color: palette.text,
                border: Border {
                    radius: 8.0.into(),
                    ..Default::default()
                },
                shadow: Shadow::default(),
                snap: false,
            },
            button::Status::Pressed => button::Style {
                background: Some(Background::Color(palette.btn_sec)),
                text_color: palette.text,
                border: Border {
                    radius: 8.0.into(),
                    ..Default::default()
                },
                shadow: Shadow::default(),
                snap: false,
            },
            button::Status::Disabled => button::Style {
                background: Some(Background::Color(Color::TRANSPARENT)),
                text_color: palette.text_dim,
                border: Border {
                    radius: 8.0.into(),
                    ..Default::default()
                },
                shadow: Shadow::default(),
                snap: false,
            },
        }
    }
}

/// 自訂 Checkbox 樣式
pub fn custom_checkbox_style(palette: ColorPalette, status: checkbox::Status) -> checkbox::Style {
    match status {
        checkbox::Status::Active { is_checked } => checkbox::Style {
            background: Background::Color(if is_checked {
                palette.primary
            } else {
                palette.input_bg
            }),
            icon_color: Color::WHITE,
            border: Border {
                radius: 4.0.into(),
                width: 1.0,
                color: if is_checked {
                    palette.primary
                } else {
                    palette.border
                },
            },
            text_color: Some(palette.text),
        },
        checkbox::Status::Hovered { is_checked } => checkbox::Style {
            background: Background::Color(if is_checked {
                palette.primary_hover
            } else {
                palette.btn_sec_hover
            }),
            icon_color: Color::WHITE,
            border: Border {
                radius: 4.0.into(),
                width: 1.0,
                color: palette.primary,
            },
            text_color: Some(palette.text),
        },
        checkbox::Status::Disabled { is_checked } => checkbox::Style {
            background: Background::Color(if is_checked {
                palette.text_dim
            } else {
                palette.input_bg
            }),
            icon_color: Color::WHITE,
            border: Border {
                radius: 4.0.into(),
                width: 1.0,
                color: palette.border,
            },
            text_color: Some(palette.text_dim),
        },
    }
}

pub fn custom_menu_style(palette: ColorPalette) -> menu::Style {
    menu::Style {
        background: Background::Color(palette.card),
        border: Border {
            radius: 8.0.into(),
            width: 1.0,
            color: palette.border,
        },
        text_color: palette.text,
        selected_text_color: palette.primary_text,
        selected_background: Background::Color(palette.primary),
        shadow: Shadow::default(),
    }
}

/// 自訂 PickList 樣式
pub fn custom_pick_list_style(
    palette: ColorPalette,
    status: pick_list::Status,
) -> pick_list::Style {
    let active_border = Border {
        radius: 6.0.into(),
        width: 1.0,
        color: palette.border,
    };
    let hover_border = Border {
        radius: 6.0.into(),
        width: 1.0,
        color: palette.primary,
    };

    match status {
        pick_list::Status::Active => pick_list::Style {
            text_color: palette.text,
            placeholder_color: palette.text_dim,
            handle_color: palette.text_dim,
            background: Background::Color(palette.input_bg),
            border: active_border,
        },
        pick_list::Status::Hovered => pick_list::Style {
            text_color: palette.text,
            placeholder_color: palette.text_dim,
            handle_color: palette.primary,
            background: Background::Color(palette.btn_sec_hover),
            border: hover_border,
        },
        pick_list::Status::Opened { .. } => pick_list::Style {
            text_color: palette.text,
            placeholder_color: palette.text_dim,
            handle_color: palette.primary,
            background: Background::Color(palette.input_bg),
            border: hover_border,
        },
    }
}

/// 自訂 TextInput 樣式
pub fn custom_text_input_style(
    palette: ColorPalette,
    status: text_input::Status,
) -> text_input::Style {
    let active_border = Border {
        radius: 6.0.into(),
        width: 1.0,
        color: palette.border,
    };
    let focus_border = Border {
        radius: 6.0.into(),
        width: 1.0,
        color: palette.primary,
    };

    match status {
        text_input::Status::Active => text_input::Style {
            background: Background::Color(palette.input_bg),
            border: active_border,
            icon: palette.text_dim,
            placeholder: palette.text_dim,
            value: palette.text,
            selection: palette.menu_selected_bg,
        },
        text_input::Status::Hovered => text_input::Style {
            background: Background::Color(palette.input_bg),
            border: focus_border,
            icon: palette.primary,
            placeholder: palette.text_dim,
            value: palette.text,
            selection: palette.menu_selected_bg,
        },
        text_input::Status::Focused { .. } => text_input::Style {
            background: Background::Color(palette.input_bg),
            border: focus_border,
            icon: palette.primary,
            placeholder: palette.text_dim,
            value: palette.text,
            selection: palette.menu_selected_bg,
        },
        text_input::Status::Disabled => text_input::Style {
            background: Background::Color(palette.btn_sec_hover),
            border: active_border,
            icon: palette.text_dim,
            placeholder: palette.text_dim,
            value: palette.text_dim,
            selection: palette.menu_selected_bg,
        },
    }
}
