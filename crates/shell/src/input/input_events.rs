use std::time::Duration;

pub const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);
pub(crate) const DOUBLE_CLICK_CELL_TOLERANCE: u16 = 1;

pub type CellPosition = (u16, u16);
pub type ShellInput = ui::InputEvent;

// Compatibility names for the shell public API. The canonical data types live in `ui`.
pub type InputKey = ui::Key;
pub type InputModifiers = ui::KeyModifiers;
pub type InputPhase = ui::InputPhase;
pub type KeyInput = ui::KeyEvent;
pub type PointerButton = ui::MouseButton;
pub type ScrollDirection = ui::ScrollDirection;
pub type MouseInput = ui::MouseEvent;
pub type InputEvent = ui::InputEvent;
