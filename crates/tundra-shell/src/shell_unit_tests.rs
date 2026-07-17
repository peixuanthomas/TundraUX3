#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };

    #[test]
    fn shell_and_lockscreen_share_the_same_session_recovery_budget() {
        let now = Instant::now();
        let mut recoveries = VecDeque::new();

        assert!(reserve_session_recovery(&mut recoveries, now));
        assert!(reserve_session_recovery(&mut recoveries, now));
        assert!(!reserve_session_recovery(&mut recoveries, now));
        assert_eq!(recoveries.len(), MAX_SESSION_RECOVERIES);
    }

    #[test]
    fn session_recovery_budget_resets_after_the_crash_loop_window() {
        let now = Instant::now();
        let mut recoveries = VecDeque::from([now, now]);
        let after_window = now + SESSION_RECOVERY_WINDOW + Duration::from_millis(1);

        assert!(reserve_session_recovery(&mut recoveries, after_window));
        assert_eq!(recoveries, VecDeque::from([after_window]));
    }

    #[test]
    fn critical_modal_preempts_and_then_restores_the_previous_modal() {
        let mut center = NotificationCenter::new("Ready");
        center.push_modal(ShellNotification::modal(
            "Normal confirmation",
            "normal",
            tundra_ui::NotificationTone::Warning,
            vec![ShellNotificationAction::new("ok", "OK")],
        ));
        center.push_critical_modal(ShellNotification::modal(
            "Recovered from panic",
            "critical",
            tundra_ui::NotificationTone::Critical,
            vec![ShellNotificationAction::new("continue", "Continue")],
        ));

        assert_eq!(
            center.active_modal_view_model().unwrap().title,
            "Recovered from panic"
        );
        center.activate_selected_action();
        assert_eq!(
            center.active_modal_view_model().unwrap().title,
            "Normal confirmation"
        );
    }

    #[test]
    fn exit_confirmation_keeps_login_as_the_content_screen() {
        let mut state = ShellState::new(ShellLaunchConfig::default(), (120, 40));
        state.screen_stack = vec![ShellScreen::Login];
        state.focused_component = ShellComponent::LoginUserList;
        state.refresh_hit_map();

        let action = state.apply_input(InputEvent::from_key_label("Esc"));

        assert_eq!(action, ShellAction::Redraw);
        assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
        assert_eq!(state.content_screen(), ShellScreen::Login);
        assert_eq!(
            state.to_shell_chrome_view_model().display_mode,
            tundra_ui::HomeDisplayMode::Auth
        );
        assert!(
            state
                .hit_map()
                .regions()
                .iter()
                .any(|region| region.component == ShellComponent::LoginUserList)
        );
        assert!(
            state
                .hit_map()
                .regions()
                .iter()
                .any(|region| region.component == ShellComponent::ExitDialog)
        );

        state.apply_input(InputEvent::from_key_label("Esc"));

        assert_eq!(state.active_screen(), ShellScreen::Login);
        assert_eq!(state.content_screen(), ShellScreen::Login);
        assert_eq!(state.focused_component(), ShellComponent::LoginUserList);
    }

    #[test]
    fn key_event_to_label_maps_requested_keys() {
        let cases = [
            (
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
                "Ctrl+C",
            ),
            (KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE), "x"),
            (KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), "Enter"),
            (KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), "Esc"),
            (
                KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
                "Backspace",
            ),
            (KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), "Tab"),
            (
                KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
                "Shift+Tab",
            ),
            (KeyEvent::new(KeyCode::Left, KeyModifiers::NONE), "Left"),
            (KeyEvent::new(KeyCode::Right, KeyModifiers::NONE), "Right"),
            (KeyEvent::new(KeyCode::Up, KeyModifiers::NONE), "Up"),
            (KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), "Down"),
            (KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE), "F(5)"),
        ];

        for (event, expected) in cases {
            assert_eq!(key_event_to_label(event), expected);
        }
    }

    #[test]
    fn mouse_event_to_input_maps_button_motion_and_scroll_events() {
        let down = mouse_event_to_input(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 7,
            modifiers: KeyModifiers::NONE,
        });
        let drag = mouse_event_to_input(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Right),
            column: 13,
            row: 8,
            modifiers: KeyModifiers::NONE,
        });
        let moved = mouse_event_to_input(MouseEvent {
            kind: MouseEventKind::Moved,
            column: 14,
            row: 9,
            modifiers: KeyModifiers::NONE,
        });
        let scroll_up = mouse_event_to_input(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 15,
            row: 10,
            modifiers: KeyModifiers::NONE,
        });

        assert_eq!(
            down,
            InputEvent::Mouse(MouseInput::Down {
                button: PointerButton::Left,
                coordinates: (12, 7),
                modifiers: InputModifiers::none(),
            })
        );
        assert_eq!(
            drag,
            InputEvent::Mouse(MouseInput::Drag {
                button: PointerButton::Right,
                coordinates: (13, 8),
                modifiers: InputModifiers::none(),
            })
        );
        assert_eq!(
            moved,
            InputEvent::Mouse(MouseInput::Moved {
                coordinates: (14, 9),
                modifiers: InputModifiers::none(),
            })
        );
        assert_eq!(
            scroll_up,
            InputEvent::Mouse(MouseInput::Scroll {
                direction: ScrollDirection::Up,
                coordinates: (15, 10),
                modifiers: InputModifiers::none(),
            })
        );
    }

    #[test]
    fn mouse_event_to_input_uses_required_scroll_direction_labels() {
        let cases = [
            (MouseEventKind::ScrollDown, "Down"),
            (MouseEventKind::ScrollUp, "Up"),
            (MouseEventKind::ScrollLeft, "Left"),
            (MouseEventKind::ScrollRight, "Right"),
        ];

        for (kind, expected_direction) in cases {
            let input = mouse_event_to_input(MouseEvent {
                kind,
                column: 1,
                row: 2,
                modifiers: KeyModifiers::NONE,
            });

            assert_eq!(
                input,
                InputEvent::Mouse(MouseInput::Scroll {
                    direction: match expected_direction {
                        "Down" => ScrollDirection::Down,
                        "Up" => ScrollDirection::Up,
                        "Left" => ScrollDirection::Left,
                        "Right" => ScrollDirection::Right,
                        _ => unreachable!("test direction"),
                    },
                    coordinates: (1, 2),
                    modifiers: InputModifiers::none(),
                })
            );
        }
    }

    #[test]
    fn platform_capability_summary_counts_native_supported_capabilities() {
        let summary = platform_capability_summary(
            PlatformKind::Windows,
            &PlatformCapabilities::native_supported(),
        );

        assert_eq!(
            summary,
            "Windows: 15 supported, 0 best-effort, 0 unsupported"
        );
    }

    #[test]
    fn notification_toast_expires_at_wall_clock_deadline() {
        let started_at = Instant::now();
        let mut notifications = NotificationCenter::new("Ready");

        notifications.notify_toast_at("Saved", started_at);
        assert_eq!(
            notifications.poll_timeout(started_at, Duration::from_millis(250)),
            Duration::from_millis(250)
        );
        assert_eq!(
            notifications.poll_timeout(
                started_at + DEFAULT_TOAST_DURATION - Duration::from_millis(100),
                Duration::from_millis(250),
            ),
            Duration::from_millis(100)
        );
        assert_eq!(
            notifications.poll_timeout(
                started_at + DEFAULT_TOAST_DURATION,
                Duration::from_millis(250),
            ),
            Duration::ZERO
        );
        notifications.expire(started_at + DEFAULT_TOAST_DURATION - Duration::from_millis(1));
        assert_eq!(notifications.toast().as_deref(), Some("Saved"));

        notifications.expire(started_at + DEFAULT_TOAST_DURATION);
        assert_eq!(notifications.toast(), None);

        let replacement_at = started_at + Duration::from_secs(10);
        notifications.notify_toast_at("First", replacement_at);
        notifications.notify_toast_at("Saved again", replacement_at + Duration::from_secs(3));
        notifications.expire(replacement_at + DEFAULT_TOAST_DURATION);
        assert_eq!(notifications.toast().as_deref(), Some("Saved again"));

        notifications.expire(replacement_at + Duration::from_secs(3) + DEFAULT_TOAST_DURATION);
        assert_eq!(notifications.toast(), None);
    }

    #[test]
    fn notification_toast_waits_behind_an_active_alert() {
        let started_at = Instant::now();
        let mut notifications = NotificationCenter::new("Ready");
        notifications.notify_alert_with_key(
            "storage",
            "Storage unavailable",
            tundra_ui::NotificationTone::Error,
        );
        notifications.notify_toast_at("Countdown finished", started_at);

        notifications.expire(started_at + DEFAULT_TOAST_DURATION + Duration::from_secs(1));

        assert_eq!(notifications.toast().as_deref(), Some("Countdown finished"));
        assert_eq!(
            notifications.poll_timeout(started_at, Duration::from_millis(250)),
            Duration::from_millis(250)
        );
        notifications.resolve_alert("storage");
        assert_eq!(notifications.toast().as_deref(), Some("Countdown finished"));
    }

    #[test]
    fn clock_storage_retry_keeps_the_due_summary_visible() {
        let mut state = ShellState::new(ShellLaunchConfig::default(), (80, 24));
        state.remember_clock_due_summary("Countdown finished".to_string());

        state.report_clock_storage_error("first failure");
        state.report_clock_storage_error("retry failure");

        assert!(
            state
                .to_shell_chrome_view_model()
                .status
                .error
                .as_deref()
                .is_some_and(|message| {
                    message.contains("Countdown finished") && message.contains("retry failure")
                })
        );
    }

    #[test]
    fn compact_clock_routes_only_escape_and_does_not_open_hidden_controls() {
        let mut state = ShellState::new(ShellLaunchConfig::default(), (49, 11));
        state.screen_stack = vec![ShellScreen::Clock];

        assert_eq!(
            state.route_clock_key(&KeyInput::from_label("n")).1,
            ShellCommand::CaptureOverlayInput
        );
        assert_eq!(state.focus_order(), vec![ShellComponent::CompactHome]);

        state.clock_create_state = Some(ClockCreateState::default());
        assert_eq!(
            state.route_clock_key(&KeyInput::from_label("Esc")).1,
            ShellCommand::ClockCloseCreate
        );
    }

    #[test]
    fn notification_alerts_resolve_by_key_and_preserve_other_sources() {
        let mut notifications = NotificationCenter::new("Ready");
        notifications.notify_alert_with_key(
            "settings",
            "Settings warning",
            tundra_ui::NotificationTone::Warning,
        );
        notifications.notify_alert_with_key(
            "explorer.operation",
            "Explorer failed",
            tundra_ui::NotificationTone::Error,
        );

        assert_eq!(notifications.alert().as_deref(), Some("Explorer failed"));
        assert_eq!(
            notifications.alert_tone(),
            Some(tundra_ui::NotificationTone::Error)
        );

        notifications.resolve_alert("explorer.operation");
        assert_eq!(notifications.alert().as_deref(), Some("Settings warning"));
        assert_eq!(
            notifications.alert_tone(),
            Some(tundra_ui::NotificationTone::Warning)
        );
    }

    #[test]
    fn notification_response_queue_is_bounded() {
        let mut notifications = NotificationCenter::new("Ready");
        let total = MAX_NOTIFICATION_RESPONSES + 5;

        for index in 0..total {
            notifications.push_modal(ShellNotification::modal(
                "Notice",
                "Continue?",
                tundra_ui::NotificationTone::Info,
                vec![ShellNotificationAction::new(format!("ok-{index}"), "OK")],
            ));
            let _follow_up = notifications.activate_selected_action();
        }

        assert_eq!(notifications.responses.len(), MAX_NOTIFICATION_RESPONSES);
        assert_eq!(
            notifications
                .responses
                .front()
                .map(|response| response.notification_id),
            Some(6)
        );
    }

    #[test]
    fn notification_follow_up_activation_is_iterative_and_bounded() {
        let mut state = ShellState::new(ShellLaunchConfig::default(), (80, 24));
        for index in 0..(MAX_NOTIFICATION_FOLLOW_UP_STEPS + 3) {
            state.notify_modal(
                format!("Notice {index}"),
                "Continue?",
                tundra_ui::NotificationTone::Info,
                vec![
                    ShellNotificationAction::new(format!("ok-{index}"), "OK")
                        .with_follow_up(ShellCommand::NotificationActivateSelected),
                ],
            );
        }

        let action = state.apply_input(InputEvent::from_key_label("Enter"));

        assert_eq!(action, ShellAction::Redraw);
        assert!(state.to_notification_view_model().is_some());
        assert_eq!(
            state.to_shell_chrome_view_model().status.error.as_deref(),
            Some("Notification follow-up limit reached")
        );
        assert_eq!(
            state.to_shell_chrome_view_model().status.alert_tone,
            tundra_ui::NotificationTone::Critical
        );
    }

    #[test]
    fn cached_time_sync_replays_into_recreated_shell_state() {
        let received_at = Instant::now();
        let consumed_at = received_at + Duration::from_secs(3);
        let replayed_at = received_at + Duration::from_secs(5);
        let utc = Utc::now();
        let mut cached = None;
        let mut original_state = ShellState::new(ShellLaunchConfig::default(), (80, 24));

        apply_timed_time_sync_result_at(
            &mut original_state,
            &mut cached,
            TimedTimeSyncResult {
                result: Ok(utc),
                received_at,
            },
            consumed_at,
        );

        assert_eq!(
            original_state.last_time_sync_utc,
            Some(utc + Duration::from_secs(3))
        );

        let mut recreated_state = ShellState::new(ShellLaunchConfig::default(), (80, 24));
        cached
            .as_ref()
            .expect("successful sync should be cached")
            .apply_to_state_at(&mut recreated_state, replayed_at);

        assert!(recreated_state.time_sync_attempted);
        assert_eq!(
            recreated_state.last_time_sync_utc,
            Some(utc + Duration::from_secs(5))
        );

        let mut failed_state = ShellState::new(ShellLaunchConfig::default(), (80, 24));
        CachedTimeSyncResult::Failure.apply_to_state_at(&mut failed_state, replayed_at);
        assert!(failed_state.time_sync_attempted);
        assert!(failed_state.time_sync_failure_dialog_visible());
    }

    #[test]
    fn auth_poll_timeout_wakes_at_password_reveal_deadline() {
        let now = Instant::now();
        let mut state = ShellState::new(ShellLaunchConfig::default(), (80, 24));
        state.screen_stack = vec![ShellScreen::Login];
        state.login_idle_deadline = now + LOGIN_IDLE_TIMEOUT;
        state.login_password_visible_until = Some(now + Duration::from_millis(10));

        assert_eq!(
            state.auth_poll_timeout(now, Duration::from_millis(250)),
            Duration::from_millis(10)
        );
    }

    #[test]
    fn startup_lockscreen_launch_options_use_storage_timezone_and_location() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "tundra-shell-lockscreen-options-{}-{nanos}",
            std::process::id()
        ));
        let app_paths = tundra_platform::build_windows_app_paths(
            base.join("Roaming"),
            base.join("Local"),
            base.join("Temp"),
        )
        .expect("app paths");
        let opened = StorageManager::open(app_paths).expect("storage opens");
        let mut config = opened.manager.load_config().expect("config loads");
        config.timezone = "Asia/Shanghai".to_string();
        opened.manager.save_config(&config).expect("config saves");

        let mut startup = ShellStartupState::clean(
            PlatformKind::Windows,
            PlatformCapabilities::native_supported(),
        );
        startup.storage_manager = Some(opened.manager.clone());

        let terminal_size_requirement = ShellTerminalSizeRequirement {
            width: 108,
            height: 20,
        };
        let options = startup_lockscreen_launch_options(&startup, terminal_size_requirement);

        assert!(!options.load_config_file);
        assert!(!options.prefer_config_location);
        assert_eq!(options.timezone_id.as_deref(), Some("Asia/Shanghai"));
        assert_eq!(options.minimum_terminal_size, Some((108, 20)));
        let location = options.location_override.expect("mapped location");
        assert_eq!(location.city.as_deref(), Some("Shanghai"));
        assert!((location.latitude - 31.2304).abs() < 0.001);
        assert!((location.longitude - 121.4737).abs() < 0.001);

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn hit_map_uses_explicit_layer_priority_instead_of_insertion_order() {
        let area = Rect::new(0, 0, 10, 5);
        let map = ShellHitMap::new(
            (10, 5),
            1,
            vec![
                ShellHitRegion {
                    component: ShellComponent::ExitDialog,
                    area,
                    layer: ShellHitLayer::ShellModal,
                },
                ShellHitRegion {
                    component: ShellComponent::ClockButton,
                    area,
                    layer: ShellHitLayer::ShellChrome,
                },
                ShellHitRegion {
                    component: ShellComponent::ContextMenu,
                    area,
                    layer: ShellHitLayer::AppOverlay,
                },
                ShellHitRegion {
                    component: ShellComponent::Explorer,
                    area,
                    layer: ShellHitLayer::AppContent,
                },
            ],
        );

        assert_eq!(map.target_at((2, 2)), Some(ShellComponent::ExitDialog));
        assert_eq!(map.layer_at((2, 2)), Some(ShellHitLayer::ShellModal));

        let without_modal = ShellHitMap::new((10, 5), 2, map.regions()[1..].to_vec());
        assert_eq!(
            without_modal.target_at((2, 2)),
            Some(ShellComponent::ClockButton)
        );

        let app_only = ShellHitMap::new((10, 5), 3, map.regions()[2..].to_vec());
        assert_eq!(
            app_only.target_at((2, 2)),
            Some(ShellComponent::ContextMenu)
        );
    }

    #[test]
    fn clock_button_routes_before_explorer_popup_and_app_forms() {
        let mut explorer = explorer_routing_test_state();
        let clock = hit_region_center(&explorer, ShellComponent::ClockButton);
        explorer.active_popup = Some(ShellPopup {
            owner: Some(ShellComponent::Explorer),
            anchor: (10, 10),
        });
        explorer.explorer_overlay_mode =
            Some(ExplorerOverlayMode::ContextMenu { anchor: (10, 10) });
        explorer.refresh_hit_map();

        let routed = explorer.route_input_at(
            InputEvent::mouse_down(PointerButton::Left, clock),
            Instant::now(),
        );
        assert_eq!(
            routed.target,
            RoutedTarget::Component(ShellComponent::ClockButton)
        );
        assert_eq!(routed.command, ShellCommand::OpenClock);

        explorer.active_popup = None;
        explorer.explorer_overlay_mode = None;
        explorer.explorer_input_mode = ExplorerInputMode::NewFolder;
        explorer.refresh_hit_map();
        let routed = explorer.route_input_at(
            InputEvent::mouse_down(PointerButton::Left, clock),
            Instant::now(),
        );
        assert_eq!(routed.command, ShellCommand::OpenClock);

        let mut user_management = ShellState::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        user_management.screen_stack = vec![ShellScreen::UserManagement];
        user_management.user_management_mode =
            UserManagementMode::Create(UserManagementCreateForm {
                username: String::new(),
                display_name: String::new(),
                password: String::new(),
                role: UserRole::User,
                focused_field: UserManagementFormField::Username,
            });
        user_management.refresh_hit_map();
        let clock = hit_region_center(&user_management, ShellComponent::ClockButton);
        let routed = user_management.route_input_at(
            InputEvent::mouse_down(PointerButton::Left, clock),
            Instant::now(),
        );
        assert_eq!(routed.command, ShellCommand::OpenClock);
    }

    #[test]
    fn clock_button_routes_outside_shell_modal_while_modal_region_stays_highest() {
        let mut state = ShellState::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        state.notify_modal(
            "Confirm",
            "Keep the clock available",
            tundra_ui::NotificationTone::Info,
            vec![ShellNotificationAction::new("ok", "OK")],
        );
        let clock = hit_region_center(&state, ShellComponent::ClockButton);
        let dialog = hit_region_center(&state, ShellComponent::NotificationDialog);

        assert_eq!(
            state.hit_map.layer_at(clock),
            Some(ShellHitLayer::ShellChrome)
        );
        assert_eq!(
            state.hit_map.layer_at(dialog),
            Some(ShellHitLayer::ShellModal)
        );

        let routed = state.route_input_at(
            InputEvent::mouse_down(PointerButton::Left, clock),
            Instant::now(),
        );
        assert_eq!(routed.command, ShellCommand::OpenClock);
    }

    #[test]
    fn explorer_never_receives_shell_chrome_pointer_commands_and_clears_drag() {
        let mut state = explorer_routing_test_state();
        state.explorer_state.as_mut().expect("Explorer state").drag =
            Some(tundra_apps::explorer::ExplorerDragState {
                sources: vec![std::path::PathBuf::from("source")],
                target: None,
                mode: tundra_apps::explorer::ExplorerTransferMode::Copy,
                active: true,
            });
        let top = hit_region_center(&state, ShellComponent::TopBar);

        let routed = state.route_input_at(
            InputEvent::mouse_drag(PointerButton::Left, top),
            Instant::now(),
        );
        assert_eq!(
            routed.target,
            RoutedTarget::Component(ShellComponent::TopBar)
        );
        assert_eq!(routed.command, ShellCommand::CaptureOverlayInput);
        assert!(
            state
                .explorer_state
                .as_ref()
                .expect("Explorer state")
                .drag
                .is_none()
        );

        let status = hit_region_center(&state, ShellComponent::StatusBar);
        for input in [
            InputEvent::mouse_down(PointerButton::Left, status),
            InputEvent::mouse_down(PointerButton::Right, status),
            InputEvent::Mouse(MouseInput::Scroll {
                direction: ScrollDirection::Down,
                coordinates: status,
                modifiers: InputModifiers::none(),
            }),
        ] {
            let routed = state.route_input_at(input, Instant::now());
            assert_eq!(
                routed.target,
                RoutedTarget::Component(ShellComponent::StatusBar)
            );
            assert_eq!(routed.command, ShellCommand::CaptureOverlayInput);
        }

        state.explorer_state.as_mut().expect("Explorer state").drag =
            Some(tundra_apps::explorer::ExplorerDragState {
                sources: vec![std::path::PathBuf::from("source")],
                target: None,
                mode: tundra_apps::explorer::ExplorerTransferMode::Move,
                active: true,
            });
        let (target, command) = state.route_explorer_mouse(
            MouseInput::Up {
                button: PointerButton::Left,
                coordinates: top,
                modifiers: InputModifiers::none(),
            },
            Some(ShellComponent::TopBar),
            Instant::now(),
        );
        assert_eq!(target, RoutedTarget::Component(ShellComponent::TopBar));
        assert_eq!(command, ShellCommand::CaptureOverlayInput);
        assert!(
            state
                .explorer_state
                .as_ref()
                .expect("Explorer state")
                .drag
                .is_none()
        );
    }

    #[test]
    fn watchdog_incident_redacts_details_and_actions_for_standard_users() {
        let mut state = ShellState::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        state.auth_session = Some(AuthSession {
            session_id: "user-session".to_string(),
            user_id: "user-id".to_string(),
            username: "user".to_string(),
            role: UserRole::User,
            started_at_epoch_ms: 1,
        });
        show_watchdog_incident(
            &mut state,
            IncidentReceipt {
                incident_id: "SECRET-INCIDENT-ID".to_string(),
                kind: tundra_watchdog::IncidentKind::Error,
                severity: tundra_watchdog::IncidentSeverity::Critical,
                app_id: None,
                component: Some("private-component".to_string()),
                task_id: None,
                task_group: None,
                boundary: "private-boundary".to_string(),
                panic_action: None,
                operation_kind: None,
                operation_id: None,
                recovery_handler_version: None,
                restart_attempt: 0,
                summary: "SECRET watchdog summary".to_string(),
                recovery: RecoveryOutcome::Recovered("SECRET recovery detail".to_string()),
                json_report_path: Some(std::path::PathBuf::from(
                    "/private/reports/SECRET-INCIDENT-ID.json",
                )),
                text_report_path: None,
            },
        );

        let modal = state.to_notification_view_model().expect("watchdog modal");
        assert!(modal.message.contains("restricted to administrators"));
        assert!(!modal.message.contains("SECRET"));
        assert!(!modal.message.contains("/private"));
        assert!(
            modal
                .actions
                .iter()
                .all(|action| { action.id != "open-report" && action.id != "copy-summary" })
        );
    }

    #[test]
    fn previous_unclean_exit_does_not_interrupt_the_login_screen() {
        let mut state = ShellState::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        state.screen_stack = vec![ShellScreen::Login];
        state.focused_component = ShellComponent::LoginUserList;
        let report_path = std::path::PathBuf::from("/reports/previous-run.txt");

        show_watchdog_incident(
            &mut state,
            IncidentReceipt {
                incident_id: "unclean-previous-run".to_string(),
                kind: IncidentKind::UncleanExit,
                severity: tundra_watchdog::IncidentSeverity::Critical,
                app_id: None,
                component: None,
                task_id: None,
                task_group: None,
                boundary: "process.unhandled".to_string(),
                panic_action: None,
                operation_kind: None,
                operation_id: None,
                recovery_handler_version: None,
                restart_attempt: 0,
                summary: "previous run ended without a clean shutdown".to_string(),
                recovery: RecoveryOutcome::Unrecoverable(
                    "the previous process had already terminated".to_string(),
                ),
                json_report_path: None,
                text_report_path: Some(report_path.clone()),
            },
        );

        assert_eq!(state.active_screen(), ShellScreen::Login);
        assert_eq!(state.focused_component(), ShellComponent::LoginUserList);
        assert!(state.to_notification_view_model().is_none());
        assert_eq!(state.latest_watchdog_report.as_ref(), Some(&report_path));
        assert!(
            state
                .latest_watchdog_summary
                .as_deref()
                .is_some_and(|summary| summary.contains("previous run ended"))
        );
    }

    fn explorer_routing_test_state() -> ShellState {
        let mut state = ShellState::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        state.screen_stack = vec![ShellScreen::Explorer];
        state.focused_component = ShellComponent::Explorer;
        state.explorer_state = Some(ExplorerState::new(".", false));
        state.refresh_hit_map();
        state
    }

    fn hit_region_center(state: &ShellState, component: ShellComponent) -> CellPosition {
        let area = state
            .hit_map
            .regions()
            .iter()
            .find(|region| region.component == component)
            .unwrap_or_else(|| panic!("missing {component:?} hit region"))
            .area;
        (
            area.x.saturating_add(area.width / 2),
            area.y.saturating_add(area.height / 2),
        )
    }
}
