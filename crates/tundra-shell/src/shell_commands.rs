use crate::{
    CellPosition, ClickKind, InputEvent, InputKey, InputModifiers, KeyInput, ShellComponent,
    ShellScreen,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellCommand {
    Noop,
    Tick,
    Shutdown,
    RequestExit,
    ConfirmExit,
    CancelExit,
    OpenLatestCrashReport,
    CopyLatestCrashSummary,
    FocusNext,
    FocusPrevious,
    AppendAuthChar(char),
    AuthBackspace,
    LoginPreviousUser,
    LoginNextUser,
    LoginPageUserUp,
    LoginPageUserDown,
    LoginFirstUser,
    LoginLastUser,
    LoginFocusUserList,
    LoginFocusPassword,
    LoginFocusPasswordVisibility,
    ToggleLoginPasswordVisibility,
    SubmitLogin,
    SubmitBootstrapAdmin,
    SetupPreviousLanguage,
    SetupNextLanguage,
    SetupContinue,
    SetupPreviousTimezone,
    SetupNextTimezone,
    SetupPageTimezoneUp,
    SetupPageTimezoneDown,
    SetupFirstTimezone,
    SetupLastTimezone,
    SetupFocusNext,
    SetupFocusPrevious,
    AppendSetupAdminChar(char),
    SetupAdminBackspace,
    SubmitSetup,
    ActivateSetup {
        target: ShellComponent,
        coordinates: CellPosition,
    },
    ActivateLogin {
        target: ShellComponent,
        coordinates: CellPosition,
    },
    HomeEntryLeft,
    HomeEntryRight,
    HomeEntryUp,
    HomeEntryDown,
    HomeFirstEntry,
    HomeLastEntry,
    ActivateSelectedHomeEntry,
    Logout,
    SelectHomeEntryAt(CellPosition),
    ActivateHomeEntryAt(CellPosition, ClickKind),
    OpenExplorer,
    CloseExplorer,
    ExplorerNext,
    ExplorerPrevious,
    ExplorerNextExtend,
    ExplorerPreviousExtend,
    ExplorerSelectAll,
    ExplorerToggleFocused,
    ExplorerOpenSelected,
    ExplorerOpenParent,
    ExplorerOpenBack,
    ExplorerOpenForward,
    ExplorerToggleHidden,
    ExplorerToggleSystem,
    ExplorerToggleExtensions,
    ExplorerToggleFoldersFirst,
    ExplorerToggleCaseSensitiveSort,
    ExplorerToggleSidebar,
    ExplorerToggleSizeFormat,
    ExplorerToggleDateZone,
    ExplorerToggleDeleteConfirmation,
    ExplorerToggleConflictConfirmation,
    ExplorerSortName,
    ExplorerSortType,
    ExplorerSortSize,
    ExplorerSortModified,
    ExplorerCopy,
    ExplorerCut,
    ExplorerPaste,
    ExplorerDelete,
    ExplorerConfirmDelete,
    ExplorerRestore,
    ExplorerDumpTrash,
    ExplorerConfirmDumpTrash,
    ExplorerRestoreKeepBoth,
    ExplorerRestoreReplace,
    ExplorerRestoreCancel,
    ExplorerConflictKeepBoth,
    ExplorerConflictReplace,
    ExplorerConflictSkip,
    ExplorerConflictCancel,
    ExplorerConflictToggleApplyToRemaining,
    ExplorerCancelOperation,
    ExplorerOverlayPrevious,
    ExplorerOverlayNext,
    ExplorerOverlayActivate,
    ExplorerSelectAt(CellPosition, ClickKind),
    ExplorerPointerDown(CellPosition, ClickKind, InputModifiers),
    ExplorerDragUpdate(CellPosition, InputModifiers),
    ExplorerDrop(CellPosition, InputModifiers),
    ExplorerCancelDrag,
    ExplorerScroll(i8),
    BeginExplorerSearch,
    BeginExplorerAddress,
    BeginExplorerNewFolder,
    BeginExplorerNewTextFile,
    BeginExplorerRename,
    AppendExplorerChar(char),
    ExplorerBackspace,
    SubmitExplorerInput,
    CancelExplorerInput,
    OpenUserManagement,
    CloseUserManagement,
    OpenClock,
    CloseClock,
    ClockOpenCreate,
    ClockCloseCreate,
    ClockCreateFocusNext,
    ClockCreateFocusPrevious,
    ClockCreateSetFocus(tundra_ui::ClockCreateDialogFocus),
    ClockCreateAppend(char),
    ClockCreateBackspace,
    ClockCreateAlarm,
    ClockCreateCountdown,
    ClockSelectPrevious,
    ClockSelectNext,
    ClockSelectPageUp,
    ClockSelectPageDown,
    ClockSelectFirst,
    ClockSelectLast,
    ClockSelectEntry(u64),
    ClockActivateSelected,
    ClockManageEntry(u64),
    ClockDeleteEntry(u64),
    ClockToggleStrong(u64),
    ClockSnoozeFiveMinutes(u64),
    UserManagementNext,
    UserManagementPrevious,
    UserManagementPageUp,
    UserManagementPageDown,
    UserManagementFirst,
    UserManagementLast,
    UserManagementSelectRow(usize),
    UserManagementFocusAction(tundra_ui::UserManagementAction),
    UserManagementActivateFocused,
    UserManagementActivateAction(tundra_ui::UserManagementAction),
    UserManagementSetFormFocus(tundra_ui::UserManagementField),
    UserManagementActivateFormControl(tundra_ui::UserManagementField),
    UserManagementToggleFormRole,
    CreateManagedUser,
    EditManagedUserInfo,
    DisableManagedUser,
    UnlockManagedUser,
    ResetManagedPassword,
    CycleManagedRole,
    RequestDeleteManagedUser,
    DeleteManagedUser,
    AppendUserManagementChar(char),
    UserManagementBackspace,
    UserManagementFocusNext,
    UserManagementFocusPrevious,
    SubmitUserManagementForm,
    CancelUserManagementForm,
    Hover(Option<ShellComponent>),
    Activate {
        target: ShellComponent,
        coordinates: CellPosition,
        click: ClickKind,
    },
    OpenContextMenu {
        target: Option<ShellComponent>,
        coordinates: CellPosition,
    },
    ClosePopup,
    CloseTimeSyncDialog,
    NotificationNextAction,
    NotificationPreviousAction,
    NotificationActivateSelected,
    NotificationActivateAction(usize),
    NotificationCancel,
    CaptureOverlayInput,
    RefreshHitMap {
        width: u16,
        height: u16,
    },
    RecordInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutedTarget {
    Global,
    Component(ShellComponent),
    Modal(ShellComponent),
    Popup(ShellComponent),
    OutsidePopup,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedEvent {
    pub input: InputEvent,
    pub target: RoutedTarget,
    pub command: ShellCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutScope {
    Global,
    Screen(ShellScreen),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBinding {
    pub key: InputKey,
    pub modifiers: InputModifiers,
}

impl From<&KeyInput> for KeyBinding {
    fn from(input: &KeyInput) -> Self {
        Self {
            key: input.key.clone(),
            modifiers: input.modifiers,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellShortcut {
    pub scope: ShortcutScope,
    pub binding: KeyBinding,
    pub command: ShellCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutConflict {
    pub scope: ShortcutScope,
    pub binding: KeyBinding,
    pub first: ShellCommand,
    pub second: ShellCommand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellAction {
    Redraw,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellTerminalFlags {
    pub raw_mode: bool,
    pub alternate_screen: bool,
    pub mouse_capture: bool,
    pub cursor_restore_enabled: bool,
}

impl ShellTerminalFlags {
    pub(crate) const fn enabled() -> Self {
        Self {
            raw_mode: true,
            alternate_screen: true,
            mouse_capture: true,
            cursor_restore_enabled: true,
        }
    }
}
