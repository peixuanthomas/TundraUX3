#[allow(clippy::too_many_arguments)]
fn build_shell_hit_map(
    terminal_size: CellPosition,
    active_screen: ShellScreen,
    active_popup: Option<ShellPopup>,
    setup_step: tundra_ui::SetupStep,
    generation: u64,
    time_button_label: Option<&str>,
    time_sync_dialog_visible: bool,
    notification_modal_component: Option<ShellComponent>,
    notification_model: Option<&tundra_ui::NotificationViewModel>,
    home_model: Option<&tundra_ui::HomeViewModel>,
    clock_model: Option<&tundra_ui::ClockViewModel>,
) -> ShellHitMap {
    let (width, height) = terminal_size;
    let area = Rect::new(0, 0, width, height);
    let mut regions = Vec::new();

    match tundra_ui::compute_shell_layout(area) {
        tundra_ui::ShellLayout::Compact(compact) => {
            regions.push(ShellHitRegion {
                component: ShellComponent::CompactHome,
                area: compact,
            });
        }
        tundra_ui::ShellLayout::Full { top, main, status } => {
            regions.push(ShellHitRegion {
                component: ShellComponent::TopBar,
                area: top,
            });
            match active_screen {
                ShellScreen::FirstRunSetup => {
                    regions.extend(setup_hit_regions(main, setup_step));
                }
                ShellScreen::Login => {
                    let layout = tundra_ui::login_layout(main);
                    regions.push(ShellHitRegion {
                        component: ShellComponent::LoginUserList,
                        area: layout.user_list,
                    });
                    regions.push(ShellHitRegion {
                        component: ShellComponent::LoginUsername,
                        area: layout.selected_username,
                    });
                    regions.push(ShellHitRegion {
                        component: ShellComponent::LoginPassword,
                        area: layout.password,
                    });
                    regions.push(ShellHitRegion {
                        component: ShellComponent::LoginPasswordVisibility,
                        area: layout.password_visibility,
                    });
                    regions.push(ShellHitRegion {
                        component: ShellComponent::LoginGuest,
                        area: layout.guest,
                    });
                }
                ShellScreen::BootstrapAdmin => {
                    let (username, password) = auth_field_rects(main);
                    regions.push(ShellHitRegion {
                        component: ShellComponent::BootstrapUsername,
                        area: username,
                    });
                    regions.push(ShellHitRegion {
                        component: ShellComponent::BootstrapPassword,
                        area: password,
                    });
                }
                ShellScreen::UserManagement => {
                    regions.push(ShellHitRegion {
                        component: ShellComponent::UserManagement,
                        area: main,
                    });
                }
                ShellScreen::Explorer => {
                    regions.push(ShellHitRegion {
                        component: ShellComponent::Explorer,
                        area: main,
                    });
                }
                ShellScreen::Clock => {
                    regions.push(ShellHitRegion {
                        component: ShellComponent::Clock,
                        area: main,
                    });
                    if let Some(model) = clock_model {
                        let layout = tundra_ui::clock_page_layout(main, model);
                        if layout.panel.width > 0 && layout.panel.height > 0 {
                            regions.push(ShellHitRegion {
                                component: ShellComponent::ClockEntryList,
                                area: layout.panel,
                            });
                        }
                        if layout.new_button.width > 0 && layout.new_button.height > 0 {
                            regions.push(ShellHitRegion {
                                component: ShellComponent::ClockNewButton,
                                area: layout.new_button,
                            });
                        }
                        regions.extend(layout.entry_rows.iter().map(|row| ShellHitRegion {
                            component: ShellComponent::ClockEntryList,
                            area: row.area,
                        }));
                        if let Some(dialog) = layout.create_dialog {
                            regions.push(ShellHitRegion {
                                component: ShellComponent::ClockCreateDialog,
                                area: dialog.dialog,
                            });
                            regions.push(ShellHitRegion {
                                component: ShellComponent::ClockCreateInput,
                                area: dialog.input,
                            });
                            regions.push(ShellHitRegion {
                                component: ShellComponent::ClockCreateAlarmButton,
                                area: dialog.create_alarm,
                            });
                            regions.push(ShellHitRegion {
                                component: ShellComponent::ClockCreateCountdownButton,
                                area: dialog.create_countdown,
                            });
                        }
                    }
                }
                _ => {
                    regions.push(ShellHitRegion {
                        component: ShellComponent::Home,
                        area: main,
                    });
                    if active_screen == ShellScreen::Home
                        && let Some(model) = home_model
                    {
                        let logout = tundra_ui::home_logout_area(main, model);
                        if logout.width > 0 && logout.height > 0 {
                            regions.push(ShellHitRegion {
                                component: ShellComponent::HomeLogout,
                                area: logout,
                            });
                        }
                    }
                }
            }
            regions.push(ShellHitRegion {
                component: ShellComponent::StatusBar,
                area: status,
            });
            if clock_button_active_for_screen(active_screen)
                && let Some(label) = time_button_label
            {
                let button = tundra_ui::status_time_button_area(status, label);
                if button.width > 0 && button.height > 0 {
                    regions.push(ShellHitRegion {
                        component: ShellComponent::ClockButton,
                        area: button,
                    });
                }
            }
        }
    }

    if let Some(popup) = active_popup {
        regions.push(ShellHitRegion {
            component: ShellComponent::ContextMenu,
            area: popup_rect(terminal_size, popup.anchor),
        });
    }

    if active_screen == ShellScreen::ExitConfirm {
        regions.push(ShellHitRegion {
            component: ShellComponent::ExitDialog,
            area: centered_rect(area, width.min(46), height.min(7)),
        });
    }

    if time_sync_dialog_visible {
        regions.push(ShellHitRegion {
            component: ShellComponent::TimeSyncDialog,
            area: centered_rect(area, width.min(34), height.min(5)),
        });
    }

    if let (Some(component), Some(model)) = (notification_modal_component, notification_model)
        && let tundra_ui::NotificationLayout::Dialog(layout) =
            tundra_ui::notification_layout(area, model)
    {
        regions.push(ShellHitRegion {
            component,
            area: layout.dialog,
        });
    }

    ShellHitMap::new(terminal_size, generation, regions)
}

fn auth_field_rects(main: Rect) -> (Rect, Rect) {
    let x = main.x.saturating_add(1);
    let width = main.width.saturating_sub(2);
    let username_y = main.y.saturating_add(3);
    let password_y = main.y.saturating_add(4);

    (
        Rect::new(x, username_y, width, 1),
        Rect::new(x, password_y, width, 1),
    )
}

fn setup_hit_regions(
    main: Rect,
    setup_step: tundra_ui::SetupStep,
) -> impl IntoIterator<Item = ShellHitRegion> {
    match setup_step {
        tundra_ui::SetupStep::Language => vec![ShellHitRegion {
            component: ShellComponent::SetupLanguage,
            area: setup_language_list_rect(main),
        }],
        tundra_ui::SetupStep::Timezone => vec![ShellHitRegion {
            component: ShellComponent::SetupTimezone,
            area: tundra_ui::setup_timezone_list_area(main),
        }],
        tundra_ui::SetupStep::Admin => vec![
            ShellHitRegion {
                component: ShellComponent::SetupAdminUsername,
                area: tundra_ui::setup_admin_field_area(main, tundra_ui::SetupField::AdminUsername),
            },
            ShellHitRegion {
                component: ShellComponent::SetupAdminPassword,
                area: tundra_ui::setup_admin_field_area(main, tundra_ui::SetupField::AdminPassword),
            },
            ShellHitRegion {
                component: ShellComponent::SetupAdminPasswordConfirm,
                area: tundra_ui::setup_admin_field_area(
                    main,
                    tundra_ui::SetupField::AdminPasswordConfirm,
                ),
            },
            ShellHitRegion {
                component: ShellComponent::SetupAdminHint,
                area: tundra_ui::setup_admin_field_area(main, tundra_ui::SetupField::PasswordHint),
            },
            ShellHitRegion {
                component: ShellComponent::SetupSubmit,
                area: tundra_ui::setup_admin_field_area(main, tundra_ui::SetupField::Submit),
            },
        ],
    }
}

fn setup_language_list_row_at(
    terminal_size: CellPosition,
    coordinates: CellPosition,
) -> Option<usize> {
    let main = setup_main_rect(terminal_size)?;
    setup_row_at(setup_language_list_rect(main), coordinates)
}

fn setup_timezone_list_row_at(
    terminal_size: CellPosition,
    coordinates: CellPosition,
) -> Option<usize> {
    let main = setup_main_rect(terminal_size)?;
    setup_row_at(tundra_ui::setup_timezone_list_area(main), coordinates)
}

fn setup_timezone_visible_row_count(terminal_size: CellPosition) -> usize {
    setup_main_rect(terminal_size)
        .map(tundra_ui::setup_timezone_visible_rows)
        .unwrap_or(0)
}

fn login_user_visible_row_count(terminal_size: CellPosition) -> usize {
    setup_main_rect(terminal_size)
        .map(tundra_ui::login_user_list_visible_rows)
        .unwrap_or(0)
}

fn setup_main_rect(terminal_size: CellPosition) -> Option<Rect> {
    let area = Rect::new(0, 0, terminal_size.0, terminal_size.1);
    let tundra_ui::ShellLayout::Full { main, .. } = tundra_ui::compute_shell_layout(area) else {
        return None;
    };

    Some(main)
}

fn setup_language_list_rect(main: Rect) -> Rect {
    tundra_ui::setup_language_list_area(main, tundra_ui::setup_language_options().len())
}

fn setup_row_at(rect: Rect, coordinates: CellPosition) -> Option<usize> {
    rect_contains(rect, coordinates).then(|| coordinates.1.saturating_sub(rect.y) as usize)
}

fn login_user_list_row_at(terminal_size: CellPosition, coordinates: CellPosition) -> Option<usize> {
    let main = setup_main_rect(terminal_size)?;
    let rect = tundra_ui::login_user_list_area(main);
    if rect.height <= 2 || !rect_contains(rect, coordinates) {
        return None;
    }

    let row = coordinates.1.checked_sub(rect.y.saturating_add(1))? as usize;
    (row < rect.height.saturating_sub(2) as usize).then_some(row)
}

fn default_login_user_index(users: &[ShellLoginUser]) -> usize {
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

fn setup_field_for_component(component: ShellComponent) -> Option<tundra_ui::SetupField> {
    match component {
        ShellComponent::SetupLanguage => Some(tundra_ui::SetupField::LanguageList),
        ShellComponent::SetupTimezone => Some(tundra_ui::SetupField::TimezoneList),
        ShellComponent::SetupAdminUsername => Some(tundra_ui::SetupField::AdminUsername),
        ShellComponent::SetupAdminPassword => Some(tundra_ui::SetupField::AdminPassword),
        ShellComponent::SetupAdminPasswordConfirm => {
            Some(tundra_ui::SetupField::AdminPasswordConfirm)
        }
        ShellComponent::SetupAdminHint => Some(tundra_ui::SetupField::PasswordHint),
        ShellComponent::SetupSubmit => Some(tundra_ui::SetupField::Submit),
        _ => None,
    }
}

fn setup_component_for_field(field: tundra_ui::SetupField) -> ShellComponent {
    match field {
        tundra_ui::SetupField::LanguageList => ShellComponent::SetupLanguage,
        tundra_ui::SetupField::TimezoneList => ShellComponent::SetupTimezone,
        tundra_ui::SetupField::AdminUsername => ShellComponent::SetupAdminUsername,
        tundra_ui::SetupField::AdminPassword => ShellComponent::SetupAdminPassword,
        tundra_ui::SetupField::AdminPasswordConfirm => ShellComponent::SetupAdminPasswordConfirm,
        tundra_ui::SetupField::PasswordHint => ShellComponent::SetupAdminHint,
        tundra_ui::SetupField::Submit => ShellComponent::SetupSubmit,
    }
}

fn setup_component_active_for_step(component: ShellComponent, step: tundra_ui::SetupStep) -> bool {
    matches!(
        (step, component),
        (
            tundra_ui::SetupStep::Language,
            ShellComponent::SetupLanguage
        ) | (
            tundra_ui::SetupStep::Timezone,
            ShellComponent::SetupTimezone
        ) | (
            tundra_ui::SetupStep::Admin,
            ShellComponent::SetupAdminUsername
                | ShellComponent::SetupAdminPassword
                | ShellComponent::SetupAdminPasswordConfirm
                | ShellComponent::SetupAdminHint
                | ShellComponent::SetupSubmit
        )
    )
}

fn setup_admin_text_field(field: tundra_ui::SetupField) -> bool {
    matches!(
        field,
        tundra_ui::SetupField::AdminUsername
            | tundra_ui::SetupField::AdminPassword
            | tundra_ui::SetupField::AdminPasswordConfirm
            | tundra_ui::SetupField::PasswordHint
    )
}

fn setup_password_requirements(
    username: &str,
    password: &str,
    password_confirm: &str,
) -> Vec<tundra_ui::SetupPasswordRequirementViewModel> {
    let normalized_username = username.trim().to_ascii_lowercase();
    let normalized_password = password.trim().to_ascii_lowercase();

    vec![
        tundra_ui::SetupPasswordRequirementViewModel::new(
            format!("At least {PASSWORD_MIN_LEN} characters"),
            password.len() >= PASSWORD_MIN_LEN,
        ),
        tundra_ui::SetupPasswordRequirementViewModel::new(
            format!("At most {PASSWORD_MAX_LEN} characters"),
            password.len() <= PASSWORD_MAX_LEN,
        ),
        tundra_ui::SetupPasswordRequirementViewModel::new("Not blank", !password.trim().is_empty()),
        tundra_ui::SetupPasswordRequirementViewModel::new(
            "Different from username",
            normalized_username != normalized_password,
        ),
        tundra_ui::SetupPasswordRequirementViewModel::new(
            "Passwords match",
            !password.is_empty() && password == password_confirm,
        ),
    ]
}

fn setup_language_code_at(
    options: &[tundra_ui::SetupLanguageOption],
    index: usize,
) -> Option<String> {
    options
        .get(index)
        .or_else(|| options.first())
        .map(|option| option.code.clone())
}

fn setup_timezone_id_at(
    options: &[tundra_ui::SetupTimezoneOption],
    index: usize,
) -> Option<String> {
    options
        .get(index)
        .or_else(|| options.first())
        .map(|option| option.id.clone())
}

fn popup_rect(terminal_size: CellPosition, anchor: CellPosition) -> Rect {
    let width = terminal_size.0.min(24);
    let height = terminal_size.1.min(5);
    let x = anchor.0.min(terminal_size.0.saturating_sub(width));
    let y = anchor.1.min(terminal_size.1.saturating_sub(height));

    Rect::new(x, y, width, height)
}

fn target_route(target: Option<ShellComponent>) -> RoutedTarget {
    target.map_or(RoutedTarget::None, RoutedTarget::Component)
}

fn rect_contains(rect: Rect, coordinates: CellPosition) -> bool {
    let (x, y) = coordinates;
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

fn coordinates_within_tolerance(first: CellPosition, second: CellPosition) -> bool {
    first.0.abs_diff(second.0) <= DOUBLE_CLICK_CELL_TOLERANCE
        && first.1.abs_diff(second.1) <= DOUBLE_CLICK_CELL_TOLERANCE
}

fn drag_direction_between(previous: CellPosition, current: CellPosition) -> Option<DragDirection> {
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
