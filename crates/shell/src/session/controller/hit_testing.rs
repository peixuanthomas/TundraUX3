use super::super::*;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::session) enum DragDirection {
    Up,
    Down,
    Left,
    Right,
}

impl DragDirection {
    pub(in crate::session) const fn label(self) -> &'static str {
        match self {
            Self::Up => "Up",
            Self::Down => "Down",
            Self::Left => "Left",
            Self::Right => "Right",
        }
    }
}
#[allow(clippy::too_many_arguments)]
pub(in crate::session) fn build_shell_hit_map(
    terminal_size: CellPosition,
    content_screen: ShellScreen,
    exit_confirmation_visible: bool,
    active_popup: Option<ShellPopup>,
    setup_step: ui::SetupStep,
    setup_custom_color_dialog_visible: bool,
    generation: u64,
    time_button_label: Option<&str>,
    time_sync_dialog_visible: bool,
    notification_modal_component: Option<ShellComponent>,
    notification_model: Option<&ui::NotificationViewModel>,
    home_model: Option<&ui::HomeViewModel>,
    clock_model: Option<&ui::ClockViewModel>,
    explorer_model: Option<&ui::ExplorerViewModel>,
    diagnostics_model: Option<&ui::DiagnosticsViewModel>,
) -> ShellHitMap {
    let (width, height) = terminal_size;
    let area = Rect::new(0, 0, width, height);
    let mut regions = Vec::new();

    match ui::compute_shell_layout(area) {
        ui::ShellLayout::Compact(compact) => {
            regions.push(ShellHitRegion {
                component: ShellComponent::CompactHome,
                area: compact,
                layer: ShellHitLayer::ShellChrome,
            });
        }
        ui::ShellLayout::Full { top, main, status } => {
            regions.push(ShellHitRegion {
                component: ShellComponent::TopBar,
                area: top,
                layer: ShellHitLayer::ShellChrome,
            });
            match content_screen {
                ShellScreen::FirstRunSetup => {
                    regions.extend(setup_hit_regions(main, setup_step));
                    if setup_custom_color_dialog_visible {
                        regions.push(ShellHitRegion {
                            component: ShellComponent::SetupCustomColorDialog,
                            area: ui::setup_custom_color_dialog_area(main),
                            layer: ShellHitLayer::AppOverlay,
                        });
                    }
                }
                ShellScreen::Login => {
                    let layout = ui::login_layout(main);
                    regions.push(ShellHitRegion {
                        component: ShellComponent::LoginUserList,
                        area: layout.user_list,
                        layer: ShellHitLayer::AppContent,
                    });
                    regions.push(ShellHitRegion {
                        component: ShellComponent::LoginUsername,
                        area: layout.selected_username,
                        layer: ShellHitLayer::AppContent,
                    });
                    regions.push(ShellHitRegion {
                        component: ShellComponent::LoginPassword,
                        area: layout.password,
                        layer: ShellHitLayer::AppContent,
                    });
                    regions.push(ShellHitRegion {
                        component: ShellComponent::LoginPasswordVisibility,
                        area: layout.password_visibility,
                        layer: ShellHitLayer::AppContent,
                    });
                }
                ShellScreen::BootstrapAdmin => {
                    let (username, password) = auth_field_rects(main);
                    regions.push(ShellHitRegion {
                        component: ShellComponent::BootstrapUsername,
                        area: username,
                        layer: ShellHitLayer::AppContent,
                    });
                    regions.push(ShellHitRegion {
                        component: ShellComponent::BootstrapPassword,
                        area: password,
                        layer: ShellHitLayer::AppContent,
                    });
                }
                ShellScreen::UserManagement => {
                    regions.push(ShellHitRegion {
                        component: ShellComponent::UserManagement,
                        area: main,
                        layer: ShellHitLayer::AppContent,
                    });
                }
                ShellScreen::Explorer => {
                    regions.push(ShellHitRegion {
                        component: ShellComponent::Explorer,
                        area: main,
                        layer: ShellHitLayer::AppContent,
                    });
                }
                ShellScreen::Launcher => {
                    regions.push(ShellHitRegion {
                        component: ShellComponent::Launcher,
                        area: main,
                        layer: ShellHitLayer::AppContent,
                    });
                }
                ShellScreen::Editor => {
                    regions.push(ShellHitRegion {
                        component: ShellComponent::Editor,
                        area: main,
                        layer: ShellHitLayer::AppContent,
                    });
                }
                ShellScreen::Settings => {
                    regions.push(ShellHitRegion {
                        component: ShellComponent::Settings,
                        area: main,
                        layer: ShellHitLayer::AppContent,
                    });
                }
                ShellScreen::Diagnostics => {
                    regions.push(ShellHitRegion {
                        component: ShellComponent::Diagnostics,
                        area: main,
                        layer: ShellHitLayer::AppContent,
                    });
                    if let Some(model) = diagnostics_model {
                        let layout = ui::diagnostics_layout(main, model);
                        if let Some(dialog) = layout.repair_dialog {
                            regions.push(ShellHitRegion {
                                component: ShellComponent::DiagnosticsRepairDialog,
                                area: dialog.dialog,
                                layer: ShellHitLayer::AppOverlay,
                            });
                        }
                    }
                }
                ShellScreen::Clock => {
                    regions.push(ShellHitRegion {
                        component: ShellComponent::Clock,
                        area: main,
                        layer: ShellHitLayer::AppContent,
                    });
                    if let Some(model) = clock_model {
                        let layout = ui::clock_page_layout(main, model);
                        if layout.panel.width > 0 && layout.panel.height > 0 {
                            regions.push(ShellHitRegion {
                                component: ShellComponent::ClockEntryList,
                                area: layout.panel,
                                layer: ShellHitLayer::AppContent,
                            });
                        }
                        if layout.new_button.width > 0 && layout.new_button.height > 0 {
                            regions.push(ShellHitRegion {
                                component: ShellComponent::ClockNewButton,
                                area: layout.new_button,
                                layer: ShellHitLayer::AppContent,
                            });
                        }
                        regions.extend(layout.entry_rows.iter().map(|row| ShellHitRegion {
                            component: ShellComponent::ClockEntryList,
                            area: row.area,
                            layer: ShellHitLayer::AppContent,
                        }));
                        if let Some(dialog) = layout.create_dialog {
                            regions.push(ShellHitRegion {
                                component: ShellComponent::ClockCreateDialog,
                                area: dialog.dialog,
                                layer: ShellHitLayer::AppOverlay,
                            });
                            regions.push(ShellHitRegion {
                                component: ShellComponent::ClockCreateInput,
                                area: dialog.input,
                                layer: ShellHitLayer::AppOverlay,
                            });
                            regions.push(ShellHitRegion {
                                component: ShellComponent::ClockCreateAlarmButton,
                                area: dialog.create_alarm,
                                layer: ShellHitLayer::AppOverlay,
                            });
                            regions.push(ShellHitRegion {
                                component: ShellComponent::ClockCreateCountdownButton,
                                area: dialog.create_countdown,
                                layer: ShellHitLayer::AppOverlay,
                            });
                        }
                    }
                }
                _ => {
                    regions.push(ShellHitRegion {
                        component: ShellComponent::Home,
                        area: main,
                        layer: ShellHitLayer::AppContent,
                    });
                    if content_screen == ShellScreen::Home
                        && let Some(model) = home_model
                    {
                        let logout = ui::home_logout_area(main, model);
                        if logout.width > 0 && logout.height > 0 {
                            regions.push(ShellHitRegion {
                                component: ShellComponent::HomeLogout,
                                area: logout,
                                layer: ShellHitLayer::AppContent,
                            });
                        }
                    }
                }
            }
            regions.push(ShellHitRegion {
                component: ShellComponent::StatusBar,
                area: status,
                layer: ShellHitLayer::ShellChrome,
            });
            if clock_button_active_for_screen(content_screen)
                && let Some(label) = time_button_label
            {
                let button = ui::status_time_button_area(status, label);
                if button.width > 0 && button.height > 0 {
                    regions.push(ShellHitRegion {
                        component: ShellComponent::ClockButton,
                        area: button,
                        layer: ShellHitLayer::ShellChrome,
                    });
                }
            }
        }
    }

    if let Some(popup) = active_popup {
        let explorer_overlay = explorer_model.and_then(|model| {
            let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(area) else {
                return None;
            };
            ui::explorer_layout(main, model)
                .overlay
                .map(|overlay| overlay.area)
        });
        regions.push(ShellHitRegion {
            component: ShellComponent::ContextMenu,
            area: explorer_overlay.unwrap_or_else(|| popup_rect(terminal_size, popup.anchor)),
            layer: ShellHitLayer::AppOverlay,
        });
    }

    if exit_confirmation_visible {
        regions.push(ShellHitRegion {
            component: ShellComponent::ExitDialog,
            area: centered_rect(area, width.min(46), height.min(7)),
            layer: ShellHitLayer::ShellModal,
        });
    }

    if time_sync_dialog_visible {
        regions.push(ShellHitRegion {
            component: ShellComponent::TimeSyncDialog,
            area: centered_rect(area, width.min(34), height.min(5)),
            layer: ShellHitLayer::ShellModal,
        });
    }

    if let (Some(component), Some(model)) = (notification_modal_component, notification_model)
        && let ui::NotificationLayout::Dialog(layout) = ui::notification_layout(area, model)
    {
        regions.push(ShellHitRegion {
            component,
            area: layout.dialog,
            layer: ShellHitLayer::ShellModal,
        });
    }

    ShellHitMap::new(terminal_size, generation, regions)
}

pub(in crate::session) fn auth_field_rects(main: Rect) -> (Rect, Rect) {
    let x = main.x.saturating_add(1);
    let width = main.width.saturating_sub(2);
    let username_y = main.y.saturating_add(3);
    let password_y = main.y.saturating_add(4);

    (
        Rect::new(x, username_y, width, 1),
        Rect::new(x, password_y, width, 1),
    )
}

pub(in crate::session) fn setup_hit_regions(
    main: Rect,
    setup_step: ui::SetupStep,
) -> impl IntoIterator<Item = ShellHitRegion> {
    match setup_step {
        ui::SetupStep::Language => vec![ShellHitRegion {
            component: ShellComponent::SetupLanguage,
            area: setup_language_list_rect(main),
            layer: ShellHitLayer::AppContent,
        }],
        ui::SetupStep::Timezone => vec![ShellHitRegion {
            component: ShellComponent::SetupTimezone,
            area: ui::setup_timezone_list_area(main),
            layer: ShellHitLayer::AppContent,
        }],
        ui::SetupStep::Admin => vec![
            ShellHitRegion {
                component: ShellComponent::SetupAdminUsername,
                area: ui::setup_admin_field_area(main, ui::SetupField::AdminUsername),
                layer: ShellHitLayer::AppContent,
            },
            ShellHitRegion {
                component: ShellComponent::SetupAdminPassword,
                area: ui::setup_admin_field_area(main, ui::SetupField::AdminPassword),
                layer: ShellHitLayer::AppContent,
            },
            ShellHitRegion {
                component: ShellComponent::SetupAdminPasswordConfirm,
                area: ui::setup_admin_field_area(main, ui::SetupField::AdminPasswordConfirm),
                layer: ShellHitLayer::AppContent,
            },
            ShellHitRegion {
                component: ShellComponent::SetupAdminHint,
                area: ui::setup_admin_field_area(main, ui::SetupField::PasswordHint),
                layer: ShellHitLayer::AppContent,
            },
            ShellHitRegion {
                component: ShellComponent::SetupSubmit,
                area: ui::setup_admin_field_area(main, ui::SetupField::Submit),
                layer: ShellHitLayer::AppContent,
            },
        ],
        ui::SetupStep::Appearance => vec![
            ShellHitRegion {
                component: ShellComponent::SetupAppearanceShape,
                area: ui::setup_appearance_field_area(main, ui::SetupField::AppearanceShape),
                layer: ShellHitLayer::AppContent,
            },
            ShellHitRegion {
                component: ShellComponent::SetupAppearanceThemeColor,
                area: ui::setup_appearance_field_area(main, ui::SetupField::AppearanceThemeColor),
                layer: ShellHitLayer::AppContent,
            },
            ShellHitRegion {
                component: ShellComponent::SetupAppearanceThemeCustom,
                area: ui::setup_appearance_field_area(main, ui::SetupField::AppearanceThemeCustom),
                layer: ShellHitLayer::AppContent,
            },
            ShellHitRegion {
                component: ShellComponent::SetupAppearanceAccentColor,
                area: ui::setup_appearance_field_area(main, ui::SetupField::AppearanceAccentColor),
                layer: ShellHitLayer::AppContent,
            },
            ShellHitRegion {
                component: ShellComponent::SetupAppearanceAccentCustom,
                area: ui::setup_appearance_field_area(main, ui::SetupField::AppearanceAccentCustom),
                layer: ShellHitLayer::AppContent,
            },
            ShellHitRegion {
                component: ShellComponent::SetupAppearanceSubmit,
                area: ui::setup_appearance_field_area(main, ui::SetupField::AppearanceSubmit),
                layer: ShellHitLayer::AppContent,
            },
        ],
    }
}

pub(in crate::session) fn setup_language_list_row_at(
    terminal_size: CellPosition,
    coordinates: CellPosition,
) -> Option<usize> {
    let main = setup_main_rect(terminal_size)?;
    setup_row_at(setup_language_list_rect(main), coordinates)
}

pub(in crate::session) fn setup_timezone_list_row_at(
    terminal_size: CellPosition,
    coordinates: CellPosition,
) -> Option<usize> {
    let main = setup_main_rect(terminal_size)?;
    setup_row_at(ui::setup_timezone_list_area(main), coordinates)
}

pub(in crate::session) fn setup_timezone_visible_row_count(terminal_size: CellPosition) -> usize {
    setup_main_rect(terminal_size)
        .map(ui::setup_timezone_visible_rows)
        .unwrap_or(0)
}

pub(in crate::session) fn login_user_visible_row_count(terminal_size: CellPosition) -> usize {
    setup_main_rect(terminal_size)
        .map(ui::login_user_list_visible_rows)
        .unwrap_or(0)
}

pub(in crate::session) fn setup_main_rect(terminal_size: CellPosition) -> Option<Rect> {
    let area = Rect::new(0, 0, terminal_size.0, terminal_size.1);
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(area) else {
        return None;
    };

    Some(main)
}

pub(in crate::session) fn setup_language_list_rect(main: Rect) -> Rect {
    ui::setup_language_list_area(main, app::setup_language_options().len())
}

pub(in crate::session) fn setup_row_at(rect: Rect, coordinates: CellPosition) -> Option<usize> {
    rect_contains(rect, coordinates).then(|| coordinates.1.saturating_sub(rect.y) as usize)
}

pub(in crate::session) fn login_user_list_row_at(
    terminal_size: CellPosition,
    coordinates: CellPosition,
) -> Option<usize> {
    let main = setup_main_rect(terminal_size)?;
    let rect = ui::login_user_list_area(main);
    if rect.height <= 2 || !rect_contains(rect, coordinates) {
        return None;
    }

    let row = coordinates.1.checked_sub(rect.y.saturating_add(1))? as usize;
    (row < rect.height.saturating_sub(2) as usize).then_some(row)
}

pub(in crate::session) fn default_login_user_index(users: &[ShellLoginUser]) -> usize {
    users
        .iter()
        .enumerate()
        .filter_map(|(index, user)| {
            user.last_login_at_epoch_ms
                .map(|last_login| (index, last_login))
        })
        .max_by_key(|(_, last_login)| *last_login)
        .map(|(index, _)| index)
        .unwrap_or(0)
}

pub(in crate::session) fn setup_field_for_component(
    component: ShellComponent,
) -> Option<ui::SetupField> {
    match component {
        ShellComponent::SetupLanguage => Some(ui::SetupField::LanguageList),
        ShellComponent::SetupTimezone => Some(ui::SetupField::TimezoneList),
        ShellComponent::SetupAdminUsername => Some(ui::SetupField::AdminUsername),
        ShellComponent::SetupAdminPassword => Some(ui::SetupField::AdminPassword),
        ShellComponent::SetupAdminPasswordConfirm => Some(ui::SetupField::AdminPasswordConfirm),
        ShellComponent::SetupAdminHint => Some(ui::SetupField::PasswordHint),
        ShellComponent::SetupSubmit => Some(ui::SetupField::Submit),
        ShellComponent::SetupAppearanceShape => Some(ui::SetupField::AppearanceShape),
        ShellComponent::SetupAppearanceThemeColor => Some(ui::SetupField::AppearanceThemeColor),
        ShellComponent::SetupAppearanceThemeCustom => Some(ui::SetupField::AppearanceThemeCustom),
        ShellComponent::SetupAppearanceAccentColor => Some(ui::SetupField::AppearanceAccentColor),
        ShellComponent::SetupAppearanceAccentCustom => Some(ui::SetupField::AppearanceAccentCustom),
        ShellComponent::SetupAppearanceSubmit => Some(ui::SetupField::AppearanceSubmit),
        _ => None,
    }
}

pub(in crate::session) fn setup_component_for_field(field: ui::SetupField) -> ShellComponent {
    match field {
        ui::SetupField::LanguageList => ShellComponent::SetupLanguage,
        ui::SetupField::TimezoneList => ShellComponent::SetupTimezone,
        ui::SetupField::AdminUsername => ShellComponent::SetupAdminUsername,
        ui::SetupField::AdminPassword => ShellComponent::SetupAdminPassword,
        ui::SetupField::AdminPasswordConfirm => ShellComponent::SetupAdminPasswordConfirm,
        ui::SetupField::PasswordHint => ShellComponent::SetupAdminHint,
        ui::SetupField::Submit => ShellComponent::SetupSubmit,
        ui::SetupField::AppearanceShape => ShellComponent::SetupAppearanceShape,
        ui::SetupField::AppearanceThemeColor => ShellComponent::SetupAppearanceThemeColor,
        ui::SetupField::AppearanceThemeCustom => ShellComponent::SetupAppearanceThemeCustom,
        ui::SetupField::AppearanceAccentColor => ShellComponent::SetupAppearanceAccentColor,
        ui::SetupField::AppearanceAccentCustom => ShellComponent::SetupAppearanceAccentCustom,
        ui::SetupField::AppearanceSubmit => ShellComponent::SetupAppearanceSubmit,
    }
}

pub(in crate::session) fn setup_component_active_for_step(
    component: ShellComponent,
    step: ui::SetupStep,
) -> bool {
    matches!(
        (step, component),
        (ui::SetupStep::Language, ShellComponent::SetupLanguage)
            | (ui::SetupStep::Timezone, ShellComponent::SetupTimezone)
            | (
                ui::SetupStep::Admin,
                ShellComponent::SetupAdminUsername
                    | ShellComponent::SetupAdminPassword
                    | ShellComponent::SetupAdminPasswordConfirm
                    | ShellComponent::SetupAdminHint
                    | ShellComponent::SetupSubmit
            )
            | (
                ui::SetupStep::Appearance,
                ShellComponent::SetupAppearanceShape
                    | ShellComponent::SetupAppearanceThemeColor
                    | ShellComponent::SetupAppearanceThemeCustom
                    | ShellComponent::SetupAppearanceAccentColor
                    | ShellComponent::SetupAppearanceAccentCustom
                    | ShellComponent::SetupAppearanceSubmit
                    | ShellComponent::SetupCustomColorDialog
            )
    )
}

pub(in crate::session) fn setup_admin_text_field(field: ui::SetupField) -> bool {
    matches!(
        field,
        ui::SetupField::AdminUsername
            | ui::SetupField::AdminPassword
            | ui::SetupField::AdminPasswordConfirm
            | ui::SetupField::PasswordHint
    )
}

pub(in crate::session) fn setup_password_requirements(
    username: &str,
    password: &str,
    password_confirm: &str,
) -> Vec<ui::SetupPasswordRequirementViewModel> {
    let normalized_username = username.trim().to_ascii_lowercase();
    let normalized_password = password.trim().to_ascii_lowercase();

    vec![
        ui::SetupPasswordRequirementViewModel::new(
            format!("At least {PASSWORD_MIN_LEN} characters"),
            password.len() >= PASSWORD_MIN_LEN,
        ),
        ui::SetupPasswordRequirementViewModel::new(
            format!("At most {PASSWORD_MAX_LEN} characters"),
            password.len() <= PASSWORD_MAX_LEN,
        ),
        ui::SetupPasswordRequirementViewModel::new("Not blank", !password.trim().is_empty()),
        ui::SetupPasswordRequirementViewModel::new(
            "Different from username",
            normalized_username != normalized_password,
        ),
        ui::SetupPasswordRequirementViewModel::new(
            "Passwords match",
            !password.is_empty() && password == password_confirm,
        ),
    ]
}

pub(in crate::session) fn setup_language_code_at(
    options: &[app::SetupLanguageOption],
    index: usize,
) -> Option<String> {
    options
        .get(index)
        .or_else(|| options.first())
        .map(|option| option.code.clone())
}

pub(in crate::session) fn setup_timezone_id_at(
    options: &[app::SetupTimezoneOption],
    index: usize,
) -> Option<String> {
    options
        .get(index)
        .or_else(|| options.first())
        .map(|option| option.id.clone())
}

pub(in crate::session) fn popup_rect(terminal_size: CellPosition, anchor: CellPosition) -> Rect {
    let width = terminal_size.0.min(24);
    let height = terminal_size.1.min(5);
    let x = anchor.0.min(terminal_size.0.saturating_sub(width));
    let y = anchor.1.min(terminal_size.1.saturating_sub(height));

    Rect::new(x, y, width, height)
}

pub(in crate::session) fn target_route(target: Option<ShellComponent>) -> RoutedTarget {
    target.map_or(RoutedTarget::None, RoutedTarget::Component)
}

pub(in crate::session) fn rect_contains(rect: Rect, coordinates: CellPosition) -> bool {
    let (x, y) = coordinates;
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

pub(in crate::session) fn coordinates_within_tolerance(
    first: CellPosition,
    second: CellPosition,
) -> bool {
    first.0.abs_diff(second.0) <= DOUBLE_CLICK_CELL_TOLERANCE
        && first.1.abs_diff(second.1) <= DOUBLE_CLICK_CELL_TOLERANCE
}

pub(in crate::session) fn drag_direction_between(
    previous: CellPosition,
    current: CellPosition,
) -> Option<DragDirection> {
    let delta_x = current.0 as i32 - previous.0 as i32;
    let delta_y = current.1 as i32 - previous.1 as i32;

    if delta_x == 0 && delta_y == 0 {
        return None;
    }

    if delta_x.abs() >= delta_y.abs() {
        if delta_x > 0 {
            Some(DragDirection::Right)
        } else {
            Some(DragDirection::Left)
        }
    } else if delta_y > 0 {
        Some(DragDirection::Down)
    } else {
        Some(DragDirection::Up)
    }
}
